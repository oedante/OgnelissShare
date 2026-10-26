// Отправить ссылку (приглашение или файлы) по почте или в мессенджере.
// На Android — системное «Поделиться»; на остальных платформах — окно
// «Скопировать сообщение / ссылку» и «Отправить по почте».

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:url_launcher/url_launcher.dart';

import '../app/app_model.dart';
import '../app/format.dart';
import '../core/models.dart';
import '../core/native.dart';
import '../l10n/l10n.dart';
import '../platform/platform_bridge.dart';
import 'shell.dart';

/// Текст сообщения со ссылкой — на языке приложения.
String linkMessage(LinkItem link, String deviceName) {
  final until = formatUntil(link.expires);
  final body = switch (link.mode) {
    'pair' => tr.msgPair(tr.deviceName(deviceName), until, link.url),
    'files' => tr.msgFiles(formatNames(link.names, link.count), until, link.url),
    _ => tr.msgOnce(until, link.url),
  };
  return '$body\n\n${tr.msgHelp}';
}

/// Отправить ссылку: системное меню или окно с вариантами.
Future<void> shareLink(BuildContext context, LinkItem link) async {
  final deviceName = AppScope.read(context).snapshot.me.name;
  final text = linkMessage(link, deviceName);
  if (await PlatformBridge.shareText(tr.msgSubject, text)) return;
  if (!context.mounted) return;
  await showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    builder: (_) => _LinkSheet(link: link, text: text),
  );
}

/// Создать ссылку и сразу предложить её отправить.
Future<void> createAndShareLink(
  BuildContext context,
  String mode, {
  List<String> paths = const [],
  bool takeFiles = false,
}) async {
  final model = AppScope.read(context);
  final LinkItem link;
  try {
    link = await model.createLink(mode, paths: paths, takeFiles: takeFiles);
  } on CoreError catch (e) {
    showSnack(tr.error(e.code));
    return;
  }
  if (!context.mounted) return;
  await shareLink(context, link);
}

class _LinkSheet extends StatelessWidget {
  const _LinkSheet({required this.link, required this.text});

  final LinkItem link;
  final String text;

  Future<void> _copy(BuildContext context, String value) async {
    await Clipboard.setData(ClipboardData(text: value));
    if (context.mounted) Navigator.of(context).pop();
    showSnack(tr.copied, duration: const Duration(seconds: 2));
  }

  Future<void> _email(BuildContext context) async {
    final uri = Uri(
      scheme: 'mailto',
      query: 'subject=${Uri.encodeComponent(tr.msgSubject)}&body=${Uri.encodeComponent(text)}',
    );
    if (context.mounted) Navigator.of(context).pop();
    try {
      await launchUrl(uri);
    } catch (_) {
      await Clipboard.setData(ClipboardData(text: text));
      showSnack(tr.copied, duration: const Duration(seconds: 2));
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(8, 0, 8, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
              child: Text(tr.linkSheetTitle, style: theme.textTheme.titleMedium),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
              child: Text(
                text,
                maxLines: 6,
                overflow: TextOverflow.ellipsis,
                style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
              ),
            ),
            ListTile(
              leading: const Icon(Icons.content_copy_rounded),
              title: Text(tr.copyMessage),
              onTap: () => _copy(context, text),
            ),
            ListTile(
              leading: const Icon(Icons.link_rounded),
              title: Text(tr.copyLink),
              onTap: () => _copy(context, link.url),
            ),
            ListTile(
              leading: const Icon(Icons.mail_outline_rounded),
              title: Text(tr.sendByEmail),
              onTap: () => _email(context),
            ),
          ],
        ),
      ),
    );
  }
}
