// Настройки — их почти нет: имя устройства, папка для файлов, сведения.

import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:url_launcher/url_launcher.dart';

import '../app/app_model.dart';
import '../core/native.dart';
import '../platform/files.dart';
import '../platform/startup.dart';
import '../l10n/l10n.dart';
import 'home_screen.dart';
import 'shell.dart';
import 'widgets.dart';

const _repoUrl = 'https://github.com/oedante/OgnelissShare';
const _siteUrl = 'https://ogneliss.ru';

class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final model = AppScope.of(context);
    final me = model.snapshot.me;
    final theme = Theme.of(context);
    final muted = theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant);

    return Scaffold(
      appBar: AppBar(title: Text(tr.settings)),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
        children: [
          Narrow(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                SectionHeader(tr.thisDevice),
                Card(
                  child: ListTile(
                    leading: PeerAvatar(platform: me.platform, online: me.online),
                    title: Text(tr.deviceName(me.name)),
                    subtitle: Text(tr.deviceNameTitle),
                    trailing: const Icon(Icons.edit_outlined),
                    onTap: () => showRenameDialog(context, me.name),
                  ),
                ),
                SectionHeader(isDesktop ? tr.downloadsFolder : tr.whereFiles),
                Card(child: _DownloadsTile(model: model)),
                SectionHeader(tr.howItWorks),
                Card(
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Text(tr.howItWorksBody, style: theme.textTheme.bodyMedium),
                  ),
                ),
                SectionHeader(tr.about),
                Card(
                  child: Column(
                    children: [
                      ListTile(
                        leading: const BrandMark(size: 40),
                        title: Text(tr.appName),
                        subtitle: Text('${tr.version(me.version)} · ${tr.license}'),
                        onTap: () => showLicensePage(
                          context: context,
                          applicationName: tr.appName,
                          applicationVersion: me.version,
                          applicationIcon: const Padding(padding: EdgeInsets.all(8), child: BrandMark(size: 64)),
                          applicationLegalese: tr.legalese,
                        ),
                      ),
                      const Divider(height: 1),
                      ListTile(
                        leading: const Icon(Icons.code_rounded),
                        title: Text(tr.sourceCode),
                        subtitle: const Text(_repoUrl),
                        onTap: () => launchUrl(Uri.parse(_repoUrl), mode: LaunchMode.externalApplication),
                      ),
                      ListTile(
                        leading: const Icon(Icons.public_rounded),
                        title: Text(tr.website),
                        subtitle: const Text('ogneliss.ru'),
                        onTap: () => launchUrl(Uri.parse(_siteUrl), mode: LaunchMode.externalApplication),
                      ),
                      ListTile(
                        leading: const Icon(Icons.hub_outlined),
                        title: Text(tr.relay),
                        subtitle: Text(me.relay == 'n0' ? tr.relayPublic : me.relay),
                      ),
                      ListTile(
                        leading: const Icon(Icons.receipt_long_outlined),
                        title: Text(tr.logs),
                        onTap: () => Navigator.of(context).push(
                          MaterialPageRoute<void>(builder: (_) => const LogScreen()),
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 12),
                Text(
                  '${me.code} · ${me.id}',
                  textAlign: TextAlign.center,
                  style: muted,
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _DownloadsTile extends StatelessWidget {
  const _DownloadsTile({required this.model});

  final AppModel model;

  String _mobileHint() {
    if (Platform.isIOS) return tr.whereFilesIos;
    final dir = model.snapshot.me.downloadDir;
    if (dir.contains('/Download/')) return tr.whereFilesAndroid;
    return dir;
  }

  Future<void> _change() async {
    final String? folder;
    try {
      folder = await Files.pickFolder();
    } catch (_) {
      return;
    }
    if (folder == null) return;
    try {
      await model.setDownloadDir(folder);
    } on CoreError catch (e) {
      showSnack(tr.error(e.code));
    }
  }

  @override
  Widget build(BuildContext context) {
    final me = model.snapshot.me;
    if (!isDesktop) {
      return ListTile(
        leading: const Icon(Icons.folder_outlined),
        title: Text(_mobileHint()),
        subtitle: Text(me.downloadDir, maxLines: 2, overflow: TextOverflow.ellipsis),
        onLongPress: () {
          Clipboard.setData(ClipboardData(text: me.downloadDir));
          showSnack(tr.copied, duration: const Duration(seconds: 2));
        },
      );
    }
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              const Icon(Icons.folder_outlined),
              const SizedBox(width: 12),
              Expanded(child: SelectableText(me.downloadDir)),
            ],
          ),
          const SizedBox(height: 12),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              OutlinedButton.icon(
                onPressed: () => Files.open(me.downloadDir),
                icon: const Icon(Icons.folder_open_rounded),
                label: Text(tr.openFolder),
              ),
              OutlinedButton(onPressed: _change, child: Text(tr.change)),
              if (me.downloadDirCustom)
                TextButton(onPressed: () => model.setDownloadDir(null), child: Text(tr.resetDefault)),
            ],
          ),
        ],
      ),
    );
  }
}

/// Журнал ядра — чтобы приложить к сообщению об ошибке.
class LogScreen extends StatefulWidget {
  const LogScreen({super.key});

  @override
  State<LogScreen> createState() => _LogScreenState();
}

class _LogScreenState extends State<LogScreen> {
  String? _text;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_text == null) _load();
  }

  Future<void> _load() async {
    final lines = await AppScope.read(context).logs();
    if (mounted) setState(() => _text = lines.join('\n'));
  }

  @override
  Widget build(BuildContext context) {
    final text = _text ?? '';
    return Scaffold(
      appBar: AppBar(
        title: Text(tr.logs),
        actions: [
          IconButton(
            tooltip: tr.copy,
            icon: const Icon(Icons.copy_rounded),
            onPressed: () {
              Clipboard.setData(ClipboardData(text: text));
              showSnack(tr.copied, duration: const Duration(seconds: 2));
            },
          ),
        ],
      ),
      body: SingleChildScrollView(
        padding: const EdgeInsets.all(16),
        child: SelectableText(
          _text == null ? '…' : (text.isEmpty ? '—' : text),
          style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
        ),
      ),
    );
  }
}
