// «Добавить устройство»: QR-код этого устройства. Второе устройство
// сканирует его камерой (или вставляет ссылку) — и устройства связаны.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:qr_flutter/qr_flutter.dart';

import '../app/app_model.dart';
import '../app/format.dart';
import '../app/theme.dart';
import '../core/models.dart';
import '../core/native.dart';
import '../platform/startup.dart';
import '../l10n/l10n.dart';
import 'scan_screen.dart';
import 'shell.dart';
import 'widgets.dart';

class AddDeviceScreen extends StatefulWidget {
  const AddDeviceScreen({super.key, this.mode = 'pair'});

  /// pair — моё устройство; once — разовый обмен.
  final String mode;

  @override
  State<AddDeviceScreen> createState() => _AddDeviceScreenState();
}

class _AddDeviceScreenState extends State<AddDeviceScreen> {
  late String _mode = widget.mode;
  DateTime? _expiresAt;
  Timer? _clock;
  AppModel? _model;

  /// Код создаётся (номер попытки — чтобы поздний ответ не перебил новый).
  bool _creating = true;
  int _attempt = 0;

  @override
  void initState() {
    super.initState();
    _clock = Timer.periodic(const Duration(seconds: 1), (_) {
      if (mounted) setState(() {});
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_model == null) {
      _model = AppScope.read(context);
      // После первого кадра: _create меняет состояние экрана.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _create();
      });
    }
  }

  @override
  void dispose() {
    _clock?.cancel();
    // Команда уходит в изолят ядра; ответ здесь не нужен.
    _model?.cancelInvite();
    super.dispose();
  }

  Future<void> _create() async {
    final attempt = ++_attempt;
    setState(() => _creating = true);
    Invite? invite;
    String? error;
    try {
      invite = await _model!.createInvite(_mode);
    } on CoreError catch (e) {
      error = e.code;
    }
    if (!mounted || attempt != _attempt) return;
    if (error != null) showSnack(tr.error(error));
    final expiresAt = invite == null ? null : DateTime.now().add(Duration(seconds: invite.expiresIn));
    setState(() {
      _creating = false;
      _expiresAt = expiresAt;
    });
  }

  void _setMode(String mode) {
    if (mode == _mode) return;
    _mode = mode;
    _create();
  }

  int get _left {
    final at = _expiresAt;
    if (at == null) return 0;
    return at.difference(DateTime.now()).inSeconds;
  }

  @override
  Widget build(BuildContext context) {
    final model = AppScope.of(context);
    final invite = model.snapshot.invite;
    final theme = Theme.of(context);
    final joined = invite?.joined;

    return Scaffold(
      appBar: AppBar(title: Text(tr.addTitle)),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
        children: [
          Narrow(
            maxWidth: 520,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                SegmentedButton<String>(
                  segments: [
                    ButtonSegment(value: 'pair', icon: const Icon(Icons.devices_rounded), label: Text(tr.modeMine)),
                    ButtonSegment(value: 'once', icon: const Icon(Icons.swap_horiz_rounded), label: Text(tr.modeOnce)),
                  ],
                  selected: {_mode},
                  onSelectionChanged: (s) => _setMode(s.first),
                ),
                const SizedBox(height: 12),
                Text(
                  _mode == 'pair' ? tr.modeMineBody : tr.modeOnceBody,
                  style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                  textAlign: TextAlign.center,
                ),
                const SizedBox(height: 20),
                if (joined != null)
                  _JoinedCard(joined: joined, mode: invite!.mode, onNew: _create)
                else if (_creating)
                  const _Creating()
                else if (invite == null || _left <= 0)
                  _ExpiredCard(onNew: _create)
                else
                  _QrCard(invite: invite, left: _left),
                if (canScan) ...[
                  const SizedBox(height: 24),
                  OutlinedButton.icon(
                    onPressed: () => Navigator.of(context).pushReplacement(
                      MaterialPageRoute<void>(builder: (_) => const ScanScreen()),
                    ),
                    icon: const Icon(Icons.qr_code_scanner_rounded),
                    label: Text(tr.scanCode),
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

/// Пока ядро готовит код (доли секунды) — место под него.
class _Creating extends StatelessWidget {
  const _Creating();

  @override
  Widget build(BuildContext context) {
    return const SizedBox(height: 280, child: Center(child: CircularProgressIndicator()));
  }
}

class _QrCard extends StatelessWidget {
  const _QrCard({required this.invite, required this.left});

  final Invite invite;
  final int left;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final width = MediaQuery.sizeOf(context).width;
    final size = (width - 96).clamp(180.0, 300.0).toDouble();
    return Column(
      children: [
        // QR всегда тёмный на белом: так его читает любая камера, и в тёмной теме.
        Center(
          child: Container(
            padding: const EdgeInsets.all(14),
            decoration: BoxDecoration(color: Colors.white, borderRadius: BorderRadius.circular(20)),
            child: QrImageView(
              data: invite.uri,
              version: QrVersions.auto,
              size: size,
              backgroundColor: Colors.white,
              errorCorrectionLevel: QrErrorCorrectLevel.M,
              eyeStyle: const QrEyeStyle(eyeShape: QrEyeShape.square, color: Brand.slateDark),
              dataModuleStyle: const QrDataModuleStyle(
                dataModuleShape: QrDataModuleShape.square,
                color: Brand.slateDark,
              ),
            ),
          ),
        ),
        const SizedBox(height: 12),
        Text(
          tr.validFor(formatClock(left)),
          style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
        ),
        const SizedBox(height: 16),
        TextButton.icon(
          onPressed: () {
            Clipboard.setData(ClipboardData(text: invite.uri));
            showSnack(tr.copied, duration: const Duration(seconds: 2));
          },
          icon: const Icon(Icons.copy_rounded),
          label: Text(tr.copyLink),
        ),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8),
          child: Text(
            tr.linkHint,
            textAlign: TextAlign.center,
            style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
          ),
        ),
      ],
    );
  }
}

class _JoinedCard extends StatelessWidget {
  const _JoinedCard({required this.joined, required this.mode, required this.onNew});

  final Joined joined;
  final String mode;
  final VoidCallback onNew;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final String text;
    final Widget badge;
    switch (joined.state) {
      case 'pending':
        text = tr.joinedPending(tr.deviceName(joined.name));
        badge = const SizedBox(width: 40, height: 40, child: CircularProgressIndicator(strokeWidth: 3));
      case 'declined':
        text = tr.joinedDeclined(tr.deviceName(joined.name));
        badge = Icon(Icons.block_rounded, size: 48, color: theme.colorScheme.error);
      default:
        text = mode == 'once' ? tr.joinedOnce(tr.deviceName(joined.name)) : tr.joinedDone(tr.deviceName(joined.name));
        badge = const Icon(Icons.check_circle_rounded, size: 56, color: Brand.ok);
    }
    final finished = joined.state != 'pending';
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          children: [
            PeerAvatar(platform: joined.platform, size: 64),
            const SizedBox(height: 16),
            badge,
            const SizedBox(height: 16),
            Text(text, textAlign: TextAlign.center, style: theme.textTheme.titleMedium),
            if (finished) ...[
              const SizedBox(height: 24),
              Wrap(
                spacing: 12,
                runSpacing: 12,
                alignment: WrapAlignment.center,
                children: [
                  FilledButton(onPressed: () => Navigator.of(context).pop(), child: Text(tr.done)),
                  OutlinedButton(onPressed: onNew, child: Text(tr.newCode)),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class _ExpiredCard extends StatelessWidget {
  const _ExpiredCard({required this.onNew});

  final VoidCallback onNew;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          children: [
            Icon(Icons.timer_off_outlined, size: 48, color: theme.colorScheme.onSurfaceVariant),
            const SizedBox(height: 12),
            Text(tr.codeExpired, style: theme.textTheme.titleMedium),
            const SizedBox(height: 16),
            FilledButton.icon(
              onPressed: onNew,
              icon: const Icon(Icons.refresh_rounded),
              label: Text(tr.newCode),
            ),
          ],
        ),
      ),
    );
  }
}
