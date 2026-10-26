// Главный экран: это устройство, мои устройства, передачи.

import 'package:flutter/material.dart';

import '../app/app_model.dart';
import '../app/format.dart';
import '../core/models.dart';
import '../core/native.dart';
import '../platform/files.dart';
import '../platform/startup.dart';
import '../l10n/l10n.dart';
import 'add_device_screen.dart';
import 'scan_screen.dart';
import 'send_sheet.dart';
import 'settings_screen.dart';
import 'share_link.dart';
import 'shell.dart';
import 'transfer_tile.dart';
import 'widgets.dart';

class HomeScreen extends StatelessWidget {
  const HomeScreen({super.key});

  static void openAdd(BuildContext context) =>
      Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => const AddDeviceScreen()));

  static void openScan(BuildContext context) =>
      Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => const ScanScreen()));

  /// Файлы по ссылке: выбрать файлы и отправить ссылку на них.
  static Future<void> filesByLink(BuildContext context) async {
    final List<String> paths;
    try {
      paths = await Files.pickFiles();
    } catch (_) {
      showSnack(tr.error('read'));
      return;
    }
    if (paths.isEmpty || !context.mounted) return;
    // На телефонах выбранное — временные копии: ядро заберёт их к ссылке.
    await createAndShareLink(context, 'files', paths: paths, takeFiles: isMobile);
  }

  @override
  Widget build(BuildContext context) {
    final model = AppScope.of(context);
    final snap = model.snapshot;
    final paired = [for (final p in snap.peers) if (!p.isSession) p];
    final sessions = [for (final p in snap.peers) if (p.isSession) p];
    final anyFinished = snap.transfers.any((t) => t.isFinished);

    return Scaffold(
      appBar: AppBar(
        titleSpacing: 16,
        title: Row(
          children: [
            const BrandMark(size: 30),
            const SizedBox(width: 10),
            Text(tr.appName),
          ],
        ),
        actions: [
          IconButton(
            tooltip: canScan ? tr.scanCode : tr.pasteLink,
            icon: Icon(canScan ? Icons.qr_code_scanner_rounded : Icons.link_rounded),
            onPressed: () => openScan(context),
          ),
          IconButton(
            tooltip: tr.filesByLink,
            icon: const Icon(Icons.add_link_rounded),
            onPressed: () => filesByLink(context),
          ),
          IconButton(
            tooltip: tr.settings,
            icon: const Icon(Icons.settings_outlined),
            onPressed: () => Navigator.of(context).push(
              MaterialPageRoute<void>(builder: (_) => const SettingsScreen()),
            ),
          ),
          const SizedBox(width: 4),
        ],
      ),
      floatingActionButton: snap.peers.isEmpty
          ? null
          : FloatingActionButton.extended(
              onPressed: () => openAdd(context),
              icon: const Icon(Icons.qr_code_2_rounded),
              label: Text(tr.addDevice),
            ),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 112),
        children: [
          Narrow(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _MeCard(me: snap.me),
                if (snap.peers.isEmpty) const _EmptyDevices(),
                if (paired.isNotEmpty) ...[
                  SectionHeader(tr.myDevices),
                  _DeviceGroup(peers: paired),
                ],
                if (sessions.isNotEmpty) ...[
                  SectionHeader(tr.oneTimePeers),
                  _DeviceGroup(peers: sessions),
                ],
                if (snap.links.isNotEmpty) ...[
                  SectionHeader(tr.links),
                  Card(
                    clipBehavior: Clip.antiAlias,
                    child: Column(
                      children: [
                        for (var i = 0; i < snap.links.length; i++) ...[
                          if (i > 0) const Divider(height: 1, indent: 72),
                          _LinkTile(key: ValueKey(snap.links[i].id), link: snap.links[i]),
                        ],
                      ],
                    ),
                  ),
                ],
                if (snap.transfers.isNotEmpty) ...[
                  SectionHeader(
                    tr.transfers,
                    trailing: anyFinished
                        ? TextButton(onPressed: () => model.clearTransfers(), child: Text(tr.clear))
                        : null,
                  ),
                  for (final t in snap.transfers)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 8),
                      child: TransferTile(key: ValueKey(t.id), item: t),
                    ),
                ],
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _MeCard extends StatelessWidget {
  const _MeCard({required this.me});

  final Me me;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final status = me.online ? tr.onlineInternet : tr.onlineLocal;
    return Card(
      child: ListTile(
        contentPadding: const EdgeInsets.fromLTRB(16, 8, 8, 8),
        leading: PeerAvatar(platform: me.platform, online: me.online),
        title: Text(tr.deviceName(me.name), style: theme.textTheme.titleMedium),
        subtitle: Padding(
          padding: const EdgeInsets.only(top: 2),
          child: Row(
            children: [
              Text(tr.thisDevice),
              const SizedBox(width: 6),
              CodeChip(me.code),
              const SizedBox(width: 6),
              Flexible(child: Text('· $status', overflow: TextOverflow.ellipsis)),
            ],
          ),
        ),
        trailing: IconButton(
          tooltip: tr.rename,
          icon: const Icon(Icons.edit_outlined),
          onPressed: () => showRenameDialog(context, me.name),
        ),
      ),
    );
  }
}

/// Переименовать это устройство.
Future<void> showRenameDialog(BuildContext context, String current) async {
  final model = AppScope.read(context);
  final controller = TextEditingController(text: current);
  final name = await showDialog<String>(
    context: context,
    builder: (dialogContext) => AlertDialog(
      title: Text(tr.deviceNameTitle),
      content: TextField(
        controller: controller,
        autofocus: true,
        maxLength: 40,
        decoration: InputDecoration(helperText: tr.deviceNameHint, border: const OutlineInputBorder()),
        onSubmitted: (v) => Navigator.of(dialogContext).pop(v),
      ),
      actions: [
        TextButton(onPressed: () => Navigator.of(dialogContext).pop(), child: Text(tr.cancel)),
        FilledButton(onPressed: () => Navigator.of(dialogContext).pop(controller.text), child: Text(tr.save)),
      ],
    ),
  );
  controller.dispose();
  if (name == null) return;
  try {
    await model.setName(name);
  } on CoreError catch (e) {
    showSnack(tr.error(e.code));
  }
}

class _EmptyDevices extends StatelessWidget {
  const _EmptyDevices();

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 16),
      child: Card(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(24, 28, 24, 24),
          child: Column(
            children: [
              const BrandMark(size: 112),
              const SizedBox(height: 16),
              Text(tr.emptyTitle, style: theme.textTheme.titleLarge, textAlign: TextAlign.center),
              const SizedBox(height: 8),
              Text(
                tr.emptyBody,
                style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 24),
              Wrap(
                alignment: WrapAlignment.center,
                spacing: 12,
                runSpacing: 12,
                children: [
                  FilledButton.icon(
                    onPressed: () => HomeScreen.openAdd(context),
                    icon: const Icon(Icons.qr_code_2_rounded),
                    label: Text(tr.showCode),
                  ),
                  OutlinedButton.icon(
                    onPressed: () => HomeScreen.openScan(context),
                    icon: Icon(canScan ? Icons.qr_code_scanner_rounded : Icons.link_rounded),
                    label: Text(canScan ? tr.scanCode : tr.pasteLink),
                  ),
                ],
              ),
              const SizedBox(height: 8),
              TextButton.icon(
                onPressed: () => HomeScreen.filesByLink(context),
                icon: const Icon(Icons.add_link_rounded),
                label: Text(tr.filesByLink),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _DeviceGroup extends StatelessWidget {
  const _DeviceGroup({required this.peers});

  final List<Peer> peers;

  @override
  Widget build(BuildContext context) {
    return Card(
      clipBehavior: Clip.antiAlias,
      child: Column(
        children: [
          for (var i = 0; i < peers.length; i++) ...[
            if (i > 0) const Divider(height: 1, indent: 72),
            _DeviceTile(peer: peers[i]),
          ],
        ],
      ),
    );
  }
}

class _DeviceTile extends StatelessWidget {
  const _DeviceTile({required this.peer});

  final Peer peer;

  String _status() {
    if (peer.online) return tr.online;
    if (peer.connecting) return tr.connecting;
    if (peer.isSession || peer.lastSeen <= 0) return tr.offline;
    return tr.lastSeen(formatAgo(peer.lastSeen));
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return ListTile(
      contentPadding: const EdgeInsets.fromLTRB(16, 6, 8, 6),
      leading: PeerAvatar(platform: peer.platform, online: peer.online),
      title: Row(
        children: [
          Flexible(child: Text(tr.deviceName(peer.name), overflow: TextOverflow.ellipsis)),
          const SizedBox(width: 8),
          CodeChip(peer.code),
          if (peer.isSession) ...[
            const SizedBox(width: 6),
            Tag(tr.sessionBadge),
          ],
        ],
      ),
      subtitle: Text(
        _status(),
        style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
      ),
      trailing: IconButton.filledTonal(
        tooltip: tr.send,
        icon: const Icon(Icons.send_rounded),
        onPressed: () => showSendSheet(context, peer),
      ),
      onTap: () => showSendSheet(context, peer),
    );
  }
}

/// Действующая ссылка: что, до какого времени, отправить ещё раз, отозвать.
class _LinkTile extends StatelessWidget {
  const _LinkTile({super.key, required this.link});

  final LinkItem link;

  Future<void> _revoke(BuildContext context) async {
    final model = AppScope.read(context);
    try {
      await model.revokeLink(link.id);
      showSnack(tr.linkRevoked, duration: const Duration(seconds: 2));
    } on CoreError catch (e) {
      showSnack(tr.error(e.code));
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final IconData icon = switch (link.mode) {
      'pair' => Icons.devices_rounded,
      'files' => Icons.insert_drive_file_outlined,
      _ => Icons.swap_horiz_rounded,
    };
    final String title = switch (link.mode) {
      'pair' => tr.linkPair,
      'files' => tr.linkFiles(formatNames(link.names, link.count)),
      _ => tr.linkOnce,
    };
    final details = [
      tr.linkUntil(formatUntil(link.expires)),
      if (link.isFiles) tr.linkUses(link.uses),
    ];
    return ListTile(
      contentPadding: const EdgeInsets.fromLTRB(16, 4, 4, 4),
      leading: CircleAvatar(
        backgroundColor: theme.colorScheme.secondaryContainer,
        foregroundColor: theme.colorScheme.onSecondaryContainer,
        child: Icon(icon),
      ),
      title: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
      subtitle: Text(
        details.join(' · '),
        style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          IconButton(
            tooltip: tr.shareAgain,
            icon: const Icon(Icons.send_rounded),
            onPressed: () => shareLink(context, link),
          ),
          IconButton(
            tooltip: tr.revoke,
            icon: const Icon(Icons.link_off_rounded),
            onPressed: () => _revoke(context),
          ),
        ],
      ),
    );
  }
}
