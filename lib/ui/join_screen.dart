// Подключение по отсканированному коду или ссылке из сообщения: ждём
// ответа другого устройства.

import 'dart:async';

import 'package:flutter/material.dart';

import '../app/app_model.dart';
import '../app/theme.dart';
import '../core/models.dart';
import '../core/native.dart';
import '../l10n/l10n.dart';
import 'scan_screen.dart';
import 'shell.dart';
import 'widgets.dart';

class JoinScreen extends StatefulWidget {
  const JoinScreen({super.key, required this.info});

  final JoinInfo info;

  @override
  State<JoinScreen> createState() => _JoinScreenState();
}

class _JoinScreenState extends State<JoinScreen> {
  StreamSubscription<CoreEvent>? _sub;

  /// null — ждём; true — готово; false — ошибка [_error].
  bool? _ok;
  String? _error;
  String? _name;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_sub == null) {
      final model = AppScope.read(context);
      _sub = model.events.listen(_onEvent);
      final early = model.lastJoin;
      if (early != null) _apply(early);
    }
  }

  @override
  void dispose() {
    _sub?.cancel();
    super.dispose();
  }

  void _onEvent(CoreEvent e) {
    if (!mounted) return;
    setState(() => _apply(e));
  }

  void _apply(CoreEvent e) {
    if (e.type != 'join_result' || e.peer != widget.info.peer) return;
    _ok = e.flag('ok');
    _error = e.maybe('error');
    final name = e.name;
    if (name.isNotEmpty) _name = name;
  }

  /// Ещё раз: ссылку из сообщения — сразу (она действует сутки), QR-код —
  /// отсканировать заново.
  Future<void> _retry() async {
    final info = widget.info;
    if (!info.remote) {
      await Navigator.of(context).pushReplacement(MaterialPageRoute<void>(builder: (_) => const ScanScreen()));
      return;
    }
    final model = AppScope.read(context);
    final navigator = Navigator.of(context);
    try {
      final again = await model.useInvite(info.uri);
      if (!mounted) return;
      await navigator.pushReplacement(MaterialPageRoute<void>(builder: (_) => JoinScreen(info: again)));
    } on CoreError catch (e) {
      showSnack(tr.error(e.code));
    }
  }

  @override
  Widget build(BuildContext context) {
    final info = widget.info;
    final theme = Theme.of(context);
    final name = tr.deviceName(_name ?? info.name);
    final Widget status;
    if (_ok == null) {
      status = Column(
        children: [
          Text(tr.joining(name), textAlign: TextAlign.center, style: theme.textTheme.titleMedium),
          const SizedBox(height: 24),
          if (info.remote && !info.once) ...[
            Text(
              tr.joinWaitingLink(name),
              textAlign: TextAlign.center,
              style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
            ),
            const SizedBox(height: 16),
          ],
          if (!info.once) ...[
            Text(tr.joinCheckCode, textAlign: TextAlign.center, style: theme.textTheme.bodyMedium),
            const SizedBox(height: 12),
            Text(
              info.code,
              style: theme.textTheme.displayMedium?.copyWith(
                fontWeight: FontWeight.w600,
                letterSpacing: 8,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
            const SizedBox(height: 24),
          ],
          const CircularProgressIndicator(),
        ],
      );
    } else if (_ok == true) {
      status = Column(
        children: [
          const Icon(Icons.check_circle_rounded, size: 64, color: Brand.ok),
          const SizedBox(height: 16),
          Text(
            info.files
                ? tr.joinDoneFiles(name)
                : (info.once ? tr.joinDoneOnce(name) : tr.joinDone(name)),
            textAlign: TextAlign.center,
            style: theme.textTheme.titleMedium,
          ),
          const SizedBox(height: 24),
          FilledButton(
            onPressed: () => Navigator.of(context).popUntil((r) => r.isFirst),
            child: Text(tr.toDevices),
          ),
        ],
      );
    } else {
      status = Column(
        children: [
          Icon(Icons.error_outline_rounded, size: 64, color: theme.colorScheme.error),
          const SizedBox(height: 16),
          Text(tr.error(_error), textAlign: TextAlign.center, style: theme.textTheme.titleMedium),
          const SizedBox(height: 24),
          Wrap(
            spacing: 12,
            runSpacing: 12,
            alignment: WrapAlignment.center,
            children: [
              FilledButton(onPressed: _retry, child: Text(tr.retry)),
              OutlinedButton(onPressed: () => Navigator.of(context).pop(), child: Text(tr.close)),
            ],
          ),
        ],
      );
    }

    return Scaffold(
      appBar: AppBar(title: Text(info.files ? tr.filesByLink : (info.once ? tr.modeOnce : tr.addTitle))),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(24, 32, 24, 32),
        children: [
          Narrow(
            maxWidth: 480,
            child: Column(
              children: [
                PeerAvatar(platform: info.platform, size: 72),
                const SizedBox(height: 12),
                Text(name, style: theme.textTheme.headlineSmall, textAlign: TextAlign.center),
                const SizedBox(height: 32),
                status,
              ],
            ),
          ),
        ],
      ),
    );
  }
}
