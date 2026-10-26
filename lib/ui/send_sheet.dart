// Отправка: что отправить на выбранное устройство и кому отправить то,
// что пришло через «Поделиться».

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../app/app_model.dart';
import '../app/format.dart';
import '../app/strings.dart';
import '../core/models.dart';
import '../core/native.dart';
import '../platform/files.dart';
import '../platform/platform_bridge.dart';
import '../platform/startup.dart';
import 'shell.dart';
import 'widgets.dart';

/// Меню «Отправить на …» для устройства.
Future<void> showSendSheet(BuildContext context, Peer peer) {
  return showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    isScrollControlled: true,
    builder: (sheetContext) => _SendSheet(peer: peer),
  );
}

class _SendSheet extends StatelessWidget {
  const _SendSheet({required this.peer});

  final Peer peer;

  @override
  Widget build(BuildContext context) {
    final model = AppScope.of(context);
    final current = model.snapshot.peer(peer.id) ?? peer;
    final theme = Theme.of(context);
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(8, 0, 8, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            ListTile(
              leading: PeerAvatar(platform: current.platform, online: current.online),
              title: Text(tr.sendTo(current.name), style: theme.textTheme.titleMedium),
              subtitle: Row(
                children: [
                  CodeChip(current.code),
                  const SizedBox(width: 8),
                  Flexible(child: Text(current.online ? tr.online : tr.offline)),
                ],
              ),
            ),
            if (!current.online)
              Padding(
                padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
                child: Text(tr.offlineNote, style: theme.textTheme.bodySmall),
              ),
            const Divider(),
            _Option(
              icon: Icons.insert_drive_file_outlined,
              label: tr.sendFiles,
              onTap: () => _pickAndSend(context, model, current, media: false),
            ),
            if (isMobile)
              _Option(
                icon: Icons.photo_library_outlined,
                label: tr.sendMedia,
                onTap: () => _pickAndSend(context, model, current, media: true),
              ),
            if (isDesktop)
              _Option(
                icon: Icons.folder_outlined,
                label: tr.sendFolder,
                onTap: () => _pickFolderAndSend(context, model, current),
              ),
            _Option(
              icon: Icons.notes_rounded,
              label: tr.sendText,
              onTap: () => _composeText(context, model, current),
            ),
            const Divider(),
            _Option(
              icon: current.isSession ? Icons.link_off_rounded : Icons.delete_outline_rounded,
              label: current.isSession ? tr.endSession : tr.forget,
              danger: true,
              onTap: () => _forget(context, model, current),
            ),
          ],
        ),
      ),
    );
  }
}

class _Option extends StatelessWidget {
  const _Option({required this.icon, required this.label, required this.onTap, this.danger = false});

  final IconData icon;
  final String label;
  final VoidCallback onTap;
  final bool danger;

  @override
  Widget build(BuildContext context) {
    final color = danger ? Theme.of(context).colorScheme.error : null;
    return ListTile(
      leading: Icon(icon, color: color),
      title: Text(label, style: TextStyle(color: color)),
      onTap: onTap,
    );
  }
}

void _sendPaths(AppModel model, Peer peer, List<String> paths) {
  if (paths.isEmpty) return;
  try {
    model.sendFiles(peer.id, paths);
  } on CoreError catch (e) {
    showSnack(tr.error(e.code));
  }
}

Future<void> _pickAndSend(BuildContext context, AppModel model, Peer peer, {required bool media}) async {
  Navigator.of(context).pop();
  final List<String> paths;
  try {
    paths = await Files.pickFiles(media: media);
  } catch (_) {
    showSnack(tr.error('read'));
    return;
  }
  _sendPaths(model, peer, paths);
}

Future<void> _pickFolderAndSend(BuildContext context, AppModel model, Peer peer) async {
  Navigator.of(context).pop();
  final String? folder;
  try {
    folder = await Files.pickFolder();
  } catch (_) {
    showSnack(tr.error('read'));
    return;
  }
  if (folder != null) _sendPaths(model, peer, [folder]);
}

Future<void> _composeText(BuildContext context, AppModel model, Peer peer) async {
  Navigator.of(context).pop();
  final ctx = navigatorKey.currentContext;
  if (ctx == null) return;
  final text = await showDialog<String>(context: ctx, builder: (_) => _TextComposer(peer: peer));
  if (text == null || text.trim().isEmpty) return;
  try {
    model.sendText(peer.id, text);
  } on CoreError catch (e) {
    showSnack(tr.error(e.code));
  }
}

Future<void> _forget(BuildContext context, AppModel model, Peer peer) async {
  Navigator.of(context).pop();
  final ctx = navigatorKey.currentContext;
  if (ctx == null) return;
  final yes = await showDialog<bool>(
    context: ctx,
    builder: (dialogContext) => AlertDialog(
      title: Text(peer.isSession ? tr.endSession : tr.forget),
      content: Text(tr.forgetConfirm(peer.name)),
      actions: [
        TextButton(onPressed: () => Navigator.of(dialogContext).pop(false), child: Text(tr.cancel)),
        FilledButton(
          style: FilledButton.styleFrom(
            backgroundColor: Theme.of(dialogContext).colorScheme.error,
            foregroundColor: Theme.of(dialogContext).colorScheme.onError,
          ),
          onPressed: () => Navigator.of(dialogContext).pop(true),
          child: Text(peer.isSession ? tr.endSession : tr.forget),
        ),
      ],
    ),
  );
  if (yes != true) return;
  try {
    model.removePeer(peer.id);
  } on CoreError catch (e) {
    showSnack(tr.error(e.code));
  }
}

class _TextComposer extends StatefulWidget {
  const _TextComposer({required this.peer});

  final Peer peer;

  @override
  State<_TextComposer> createState() => _TextComposerState();
}

class _TextComposerState extends State<_TextComposer> {
  final _controller = TextEditingController();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _paste() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text;
    if (text == null || !mounted) return;
    _controller.text = text;
    _controller.selection = TextSelection.collapsed(offset: text.length);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(tr.sendTo(widget.peer.name)),
      content: SizedBox(
        width: 420,
        child: TextField(
          controller: _controller,
          autofocus: true,
          minLines: 3,
          maxLines: 8,
          decoration: InputDecoration(
            hintText: tr.textHint,
            border: const OutlineInputBorder(),
            suffixIcon: IconButton(
              tooltip: tr.paste,
              icon: const Icon(Icons.content_paste_rounded),
              onPressed: _paste,
            ),
          ),
        ),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(context).pop(), child: Text(tr.cancel)),
        FilledButton(onPressed: () => Navigator.of(context).pop(_controller.text), child: Text(tr.send)),
      ],
    );
  }
}

/// «Кому отправить?» — для того, что пришло через «Поделиться».
Future<void> showShareTargetSheet(BuildContext context, SharedPayload payload) {
  return showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    isScrollControlled: true,
    builder: (sheetContext) => _ShareTargets(payload: payload),
  );
}

class _ShareTargets extends StatelessWidget {
  const _ShareTargets({required this.payload});

  final SharedPayload payload;

  @override
  Widget build(BuildContext context) {
    final model = AppScope.of(context);
    final peers = model.snapshot.peers;
    final theme = Theme.of(context);
    final what = payload.files.isNotEmpty
        ? formatNames([for (final f in payload.files.take(2)) f.split(RegExp(r'[\\/]')).last], payload.files.length)
        : (payload.text ?? '').trim();
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(8, 0, 8, 16),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 4),
              child: Text(tr.shareChoose, style: theme.textTheme.titleMedium),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
              child: Text(what, maxLines: 2, overflow: TextOverflow.ellipsis, style: theme.textTheme.bodySmall),
            ),
            if (peers.isEmpty)
              Padding(padding: const EdgeInsets.all(16), child: Text(tr.shareNoDevices))
            else
              Flexible(
                child: ListView(
                  shrinkWrap: true,
                  children: [
                    for (final p in peers)
                      ListTile(
                        leading: PeerAvatar(platform: p.platform, online: p.online),
                        title: Text(p.name),
                        subtitle: Text(p.online ? tr.online : tr.offline),
                        trailing: CodeChip(p.code),
                        onTap: () {
                          Navigator.of(context).pop();
                          if (payload.files.isNotEmpty) {
                            _sendPaths(model, p, payload.files);
                          } else if (payload.text != null) {
                            try {
                              model.sendText(p.id, payload.text!);
                            } on CoreError catch (e) {
                              showSnack(tr.error(e.code));
                            }
                          }
                        },
                      ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}
