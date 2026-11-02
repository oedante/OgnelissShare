// «Без приложения»: большой QR-код с адресом в локальной сети. Получатель
// наводит камеру, открывает страницу в браузере и скачивает файлы прямо с
// этого устройства. Ни серверов, ни установки.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../app/app_model.dart';
import '../app/format.dart';
import '../app/icons.dart';
import '../app/theme.dart';
import '../core/models.dart';
import '../core/native.dart';
import '../l10n/l10n.dart';
import 'share_link.dart';
import 'shell.dart';
import 'widgets.dart';

/// Как отдать выбранные файлы: QR-код для браузера или ссылка для приложения.
Future<void> showGiveFilesSheet(BuildContext context, List<String> paths, {required bool takeFiles}) {
  return showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    useSafeArea: true,
    useRootNavigator: true,
    builder: (sheetContext) {
      final theme = Theme.of(sheetContext);
      void pick(Future<void> Function(BuildContext) action) {
        Navigator.of(sheetContext).pop();
        final ctx = navigatorKey.currentContext;
        if (ctx != null) action(ctx);
      }

      return SafeArea(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(8, 0, 8, 16),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                child: Text(tr.giveFilesHow, style: theme.textTheme.titleMedium),
              ),
              ListTile(
                leading: const Icon(AppIcons.qr_code_2),
                title: Text(tr.webQr),
                subtitle: Text(tr.webQrHint),
                onTap: () => pick((ctx) => startWebShare(ctx, paths, takeFiles: takeFiles)),
              ),
              ListTile(
                leading: const Icon(AppIcons.add_link),
                title: Text(tr.filesByLink),
                subtitle: Text(tr.filesByLinkApp),
                onTap: () => pick((ctx) => createAndShareLink(ctx, 'files', paths: paths, takeFiles: takeFiles)),
              ),
            ],
          ),
        ),
      );
    },
  );
}

/// Начать раздачу и показать код.
Future<void> startWebShare(BuildContext context, List<String> paths, {bool takeFiles = false}) async {
  final model = AppScope.read(context);
  final WebShare share;
  try {
    share = await model.startWebShare(paths, takeFiles: takeFiles);
  } on CoreError catch (e) {
    showSnack(tr.error(e.code));
    return;
  }
  if (!context.mounted) return;
  await openWebShare(context, share.id);
}

Future<void> openWebShare(BuildContext context, String id) => Navigator.of(context).push(
      MaterialPageRoute<void>(builder: (_) => WebShareScreen(id: id)),
    );

class WebShareScreen extends StatefulWidget {
  const WebShareScreen({super.key, required this.id});

  final String id;

  @override
  State<WebShareScreen> createState() => _WebShareScreenState();
}

class _WebShareScreenState extends State<WebShareScreen> {
  Timer? _clock;
  int _address = 0;

  /// Последнее известное состояние: после окончания раздачи экран не пустеет.
  WebShare? _last;

  @override
  void initState() {
    super.initState();
    _clock = Timer.periodic(const Duration(seconds: 1), (_) {
      if (mounted) setState(() {});
    });
  }

  @override
  void dispose() {
    _clock?.cancel();
    super.dispose();
  }

  Future<void> _stop() async {
    try {
      await AppScope.read(context).stopWebShare(widget.id);
    } on CoreError catch (e) {
      if (e.code != 'not_found') showSnack(tr.error(e.code));
    }
  }

  Future<void> _copy(String url) async {
    await Clipboard.setData(ClipboardData(text: url));
    showSnack(tr.copied, duration: const Duration(seconds: 2));
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final live = AppScope.of(context).snapshot.webShare(widget.id);
    final share = live ?? _last;
    _last = share;
    final left = share == null ? 0 : share.expires - DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final ended = live == null || left <= 0;
    final urls = share?.urls ?? const <String>[];
    final url = urls.isEmpty ? '' : urls[_address % urls.length];
    final muted = theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant);

    return Scaffold(
      appBar: AppBar(title: Text(tr.webTitle)),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
        children: [
          Narrow(
            maxWidth: 520,
            child: Card(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(20, 20, 20, 16),
                child: Column(
                  children: [
                    if (share != null)
                      Text(
                        '${formatNames(share.names, share.count)} · ${formatBytes(share.total)}',
                        textAlign: TextAlign.center,
                        style: theme.textTheme.titleMedium,
                      ),
                    const SizedBox(height: 16),
                    if (ended)
                      _Ended(onClose: () => Navigator.of(context).maybePop())
                    else if (url.isEmpty)
                      _NoNetwork(muted: muted)
                    else ...[
                      Text(tr.webScan, style: theme.textTheme.bodyMedium),
                      const SizedBox(height: 12),
                      _BigQr(data: url),
                      const SizedBox(height: 12),
                      InkWell(
                        borderRadius: BorderRadius.circular(8),
                        onTap: () => _copy(url),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                          child: Text(
                            url,
                            textAlign: TextAlign.center,
                            style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.primary),
                          ),
                        ),
                      ),
                      const SizedBox(height: 4),
                      Text('${tr.webValid(formatClock(left))} · ${tr.webDownloads(share!.downloads)}', style: muted),
                    ],
                  ],
                ),
              ),
            ),
          ),
          if (!ended) ...[
            const SizedBox(height: 16),
            Narrow(
              maxWidth: 520,
              child: Wrap(
                alignment: WrapAlignment.center,
                spacing: 12,
                runSpacing: 8,
                children: [
                  if (url.isNotEmpty)
                    OutlinedButton.icon(
                      onPressed: () => _copy(url),
                      icon: const Icon(AppIcons.content_copy),
                      label: Text(tr.copyLink),
                    ),
                  if (urls.length > 1)
                    OutlinedButton.icon(
                      onPressed: () => setState(() => _address++),
                      icon: const Icon(AppIcons.swap_horiz),
                      label: Text(tr.webOtherAddress),
                    ),
                  FilledButton.tonalIcon(
                    onPressed: _stop,
                    icon: const Icon(AppIcons.link_off),
                    label: Text(tr.webStop),
                  ),
                ],
              ),
            ),
            const SizedBox(height: 16),
            Narrow(
              maxWidth: 520,
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 8),
                child: Text(tr.webSameNetwork, textAlign: TextAlign.center, style: muted),
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class _BigQr extends StatelessWidget {
  const _BigQr({required this.data});

  final String data;

  @override
  Widget build(BuildContext context) {
    final width = MediaQuery.sizeOf(context).width;
    final size = (width - 112).clamp(180.0, 320.0).toDouble();
    // Тёмный на белом — так код читает любая камера, и в тёмной теме.
    return Container(
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(color: Colors.white, borderRadius: BorderRadius.circular(20)),
      child: QrImageView(
        data: data,
        version: QrVersions.auto,
        size: size,
        backgroundColor: Colors.white,
        errorCorrectionLevel: QrErrorCorrectLevel.M,
        eyeStyle: const QrEyeStyle(eyeShape: QrEyeShape.square, color: Brand.slateDark),
        dataModuleStyle: const QrDataModuleStyle(dataModuleShape: QrDataModuleShape.square, color: Brand.slateDark),
      ),
    );
  }
}

class _NoNetwork extends StatelessWidget {
  const _NoNetwork({required this.muted});

  final TextStyle? muted;

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Icon(AppIcons.wifi_off, size: 48, color: Theme.of(context).colorScheme.onSurfaceVariant),
        const SizedBox(height: 12),
        Text(tr.webNoNetwork, textAlign: TextAlign.center, style: muted),
      ],
    );
  }
}

class _Ended extends StatelessWidget {
  const _Ended({required this.onClose});

  final VoidCallback onClose;

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        Icon(AppIcons.timer_off, size: 48, color: Theme.of(context).colorScheme.onSurfaceVariant),
        const SizedBox(height: 12),
        Text(tr.webEnded, style: Theme.of(context).textTheme.titleMedium),
        const SizedBox(height: 16),
        FilledButton(onPressed: onClose, child: Text(tr.close)),
      ],
    );
  }
}

/// Действующая раздача в списке главного экрана.
class WebShareTile extends StatelessWidget {
  const WebShareTile({super.key, required this.share});

  final WebShare share;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final left = share.expires - DateTime.now().millisecondsSinceEpoch ~/ 1000;
    return ListTile(
      contentPadding: const EdgeInsets.fromLTRB(16, 4, 4, 4),
      leading: CircleAvatar(
        backgroundColor: theme.colorScheme.secondaryContainer,
        foregroundColor: theme.colorScheme.onSecondaryContainer,
        child: const Icon(AppIcons.qr_code_2),
      ),
      title: Text(formatNames(share.names, share.count), maxLines: 1, overflow: TextOverflow.ellipsis),
      subtitle: Text(
        '${tr.webValid(formatDuration(left < 0 ? 0 : left))} · ${tr.webDownloads(share.downloads)}',
        style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
      ),
      onTap: () => openWebShare(context, share.id),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            tooltip: tr.webShow,
            icon: const Icon(AppIcons.qr_code_2),
            onPressed: () => openWebShare(context, share.id),
          ),
          IconButton(
            tooltip: tr.webStop,
            icon: const Icon(AppIcons.link_off),
            onPressed: () async {
              try {
                await AppScope.read(context).stopWebShare(share.id);
              } on CoreError catch (e) {
                showSnack(tr.error(e.code));
              }
            },
          ),
        ],
      ),
    );
  }
}
