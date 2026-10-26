// Оболочка главного экрана: вопросы «Подключить?» и «Принять файлы?»,
// уведомления о полученном и «Поделиться» из других приложений.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app/app_model.dart';
import '../app/format.dart';
import '../app/strings.dart';
import '../core/models.dart';
import '../platform/files.dart';
import '../platform/platform_bridge.dart';
import 'send_sheet.dart';
import 'widgets.dart';

final navigatorKey = GlobalKey<NavigatorState>();
final messengerKey = GlobalKey<ScaffoldMessengerState>();

void showSnack(String text, {String? action, VoidCallback? onAction, Duration? duration}) {
  final messenger = messengerKey.currentState;
  if (messenger == null) return;
  messenger.hideCurrentSnackBar();
  messenger.showSnackBar(
    SnackBar(
      content: Text(text),
      duration: duration ?? const Duration(seconds: 4),
      action: action == null ? null : SnackBarAction(label: action, onPressed: onAction ?? () {}),
    ),
  );
}

class Shell extends StatefulWidget {
  const Shell({super.key, required this.child});

  final Widget child;

  @override
  State<Shell> createState() => _ShellState();
}

class _ShellState extends State<Shell> {
  AppModel? _model;
  StreamSubscription<CoreEvent>? _sub;
  final _handled = <String>{};
  bool _dialogOpen = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final model = AppScope.of(context);
    if (!identical(model, _model)) {
      _sub?.cancel();
      _model = model;
      _sub = model.events.listen(_onEvent);
      PlatformBridge.listen(onPreparing: _onSharing, onShared: _takeShare);
      WidgetsBinding.instance.addPostFrameCallback((_) => _takeShare());
    }
    WidgetsBinding.instance.addPostFrameCallback((_) => _checkRequests());
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;

  // ------------------------------------------------------------ вопросы

  void _checkRequests() {
    final model = _model;
    if (!mounted || model == null || _dialogOpen) return;
    for (final r in model.snapshot.requests) {
      if (_handled.contains(r.id)) continue;
      _handled.add(r.id);
      _dialogOpen = true;
      _ask(model, r).whenComplete(() {
        _dialogOpen = false;
        if (mounted) _checkRequests();
      });
      return;
    }
  }

  Future<void> _ask(AppModel model, Request r) async {
    final ctx = navigatorKey.currentContext;
    if (ctx == null) return;
    final answer = await showDialog<bool>(
      context: ctx,
      barrierDismissible: false,
      builder: (_) => RequestDialog(model: model, request: r),
    );
    // null — окно закрыто кнопкой «Назад» (считаем отказом) или вопрос снят
    // ядром (время вышло, отправитель передумал) — тогда ответ ни на что не влияет.
    final accept = answer ?? false;
    if (r.kind == 'pair') {
      model.respondPair(r.id, accept);
    } else {
      model.respondOffer(r.id, accept);
    }
  }

  // ------------------------------------------------------------ события

  void _onEvent(CoreEvent e) {
    if (!mounted) return;
    switch (e.type) {
      case 'received':
        final saved = e.maybe('saved_to');
        final failed = e.number('failed');
        var text = tr.receivedFiles(e.name, e.number('count') - failed);
        if (failed > 0) text = '$text · ${tr.failedFiles(failed)}';
        showSnack(
          text,
          action: saved == null ? null : tr.open,
          onAction: saved == null ? null : () => Files.open(saved),
          duration: const Duration(seconds: 6),
        );
      case 'text':
        _showText(e);
      case 'peer_removed':
        showSnack(tr.peerRemoved(e.name));
    }
  }

  Future<void> _showText(CoreEvent e) async {
    final ctx = navigatorKey.currentContext;
    final model = _model;
    if (ctx == null || model == null) return;
    final text = model.fullText(e.str('transfer')) ?? e.str('text');
    await showDialog<void>(
      context: ctx,
      builder: (_) => TextDialog(title: tr.receivedText(e.name), text: text),
    );
  }

  // ------------------------------------------------------------ «Поделиться»

  void _onSharing(int count) {
    if (count > 0) showSnack(tr.preparingShare(count), duration: const Duration(seconds: 2));
  }

  Future<void> _takeShare() async {
    final payload = await PlatformBridge.takeShared();
    if (payload == null || !mounted) return;
    final ctx = navigatorKey.currentContext;
    if (ctx == null) return;
    await showShareTargetSheet(ctx, payload);
  }
}

/// «Подключить устройство?» или «Принять файлы?». Закрывается сам, если
/// вопрос исчез из состояния ядра (истекло время, отправитель отменил).
class RequestDialog extends StatefulWidget {
  const RequestDialog({super.key, required this.model, required this.request});

  final AppModel model;
  final Request request;

  @override
  State<RequestDialog> createState() => _RequestDialogState();
}

class _RequestDialogState extends State<RequestDialog> {
  bool _closed = false;

  @override
  void initState() {
    super.initState();
    widget.model.addListener(_onChange);
  }

  @override
  void dispose() {
    widget.model.removeListener(_onChange);
    super.dispose();
  }

  void _onChange() {
    if (_closed || !mounted) return;
    final still = widget.model.snapshot.requests.any((r) => r.id == widget.request.id);
    if (!still) _close(null);
  }

  void _close(bool? answer) {
    if (_closed) return;
    _closed = true;
    Navigator.of(context).pop(answer);
  }

  @override
  Widget build(BuildContext context) {
    final r = widget.request;
    final theme = Theme.of(context);
    if (r.kind == 'pair') {
      return AlertDialog(
        icon: PeerAvatar(platform: r.platform),
        title: Text(tr.pairRequestTitle(r.name), textAlign: TextAlign.center),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              r.code,
              style: theme.textTheme.displayMedium?.copyWith(
                fontWeight: FontWeight.w600,
                letterSpacing: 8,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
            const SizedBox(height: 12),
            Text(tr.pairRequestBody, textAlign: TextAlign.center),
          ],
        ),
        actions: [
          TextButton(onPressed: () => _close(false), child: Text(tr.decline)),
          FilledButton(onPressed: () => _close(true), child: Text(tr.connect)),
        ],
      );
    }
    return AlertDialog(
      icon: PeerAvatar(platform: widget.model.snapshot.peer(r.peer)?.platform ?? ''),
      title: Text(tr.offerTitle(r.name), textAlign: TextAlign.center),
      content: Text(
        '${formatNames(r.names, r.count)}\n${formatBytes(r.total)}',
        textAlign: TextAlign.center,
      ),
      actions: [
        TextButton(onPressed: () => _close(false), child: Text(tr.decline)),
        FilledButton(onPressed: () => _close(true), child: Text(tr.accept)),
      ],
    );
  }
}

/// Полученный текст с кнопкой «Копировать».
class TextDialog extends StatelessWidget {
  const TextDialog({super.key, required this.title, required this.text});

  final String title;
  final String text;

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(title),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 360, minWidth: 280),
        child: SingleChildScrollView(child: SelectableText(text)),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(tr.close)),
        FilledButton.icon(
          onPressed: () {
            Clipboard.setData(ClipboardData(text: text));
            Navigator.of(context).pop();
            showSnack(tr.copied, duration: const Duration(seconds: 2));
          },
          icon: const Icon(Icons.copy_rounded),
          label: Text(tr.copy),
        ),
      ],
    );
  }
}
