// «Сканировать код»: камера (телефоны, Mac) и поле для вставки ссылки.

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import '../app/app_model.dart';
import '../core/native.dart';
import '../platform/startup.dart';
import '../l10n/l10n.dart';
import 'join_screen.dart';
import 'widgets.dart';

const _scheme = 'ogshare://';

class ScanScreen extends StatefulWidget {
  const ScanScreen({super.key});

  @override
  State<ScanScreen> createState() => _ScanScreenState();
}

class _ScanScreenState extends State<ScanScreen> {
  final _link = TextEditingController();
  MobileScannerController? _camera;
  bool _busy = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    if (canScan) {
      _camera = MobileScannerController(formats: const [BarcodeFormat.qrCode]);
    }
  }

  @override
  void dispose() {
    _link.dispose();
    _camera?.dispose();
    super.dispose();
  }

  void _onDetect(BarcodeCapture capture) {
    if (_busy) return;
    for (final code in capture.barcodes) {
      final value = code.rawValue;
      if (value != null && value.startsWith(_scheme)) {
        _use(value);
        return;
      }
    }
  }

  Future<void> _paste() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text?.trim();
    if (text == null || text.isEmpty || !mounted) return;
    _link.text = text;
    _use(text);
  }

  Future<void> _use(String uri) async {
    if (_busy) return;
    final model = AppScope.read(context);
    final navigator = Navigator.of(context);
    setState(() {
      _busy = true;
      _error = null;
    });
    final JoinInfo info;
    try {
      info = await model.useInvite(uri);
    } on CoreError catch (e) {
      if (!mounted) return;
      setState(() {
        _busy = false;
        _error = tr.error(e.code);
      });
      return;
    }
    if (!mounted) return;
    unawaited(_camera?.stop());
    unawaited(navigator.pushReplacement(MaterialPageRoute<void>(builder: (_) => JoinScreen(info: info))));
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final camera = _camera;
    return Scaffold(
      appBar: AppBar(title: Text(canScan ? tr.scanTitle : tr.pasteLink)),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 32),
        children: [
          Narrow(
            maxWidth: 520,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (camera != null) ...[
                  AspectRatio(
                    aspectRatio: 1,
                    child: ClipRRect(
                      borderRadius: BorderRadius.circular(20),
                      child: Stack(
                        fit: StackFit.expand,
                        children: [
                          MobileScanner(
                            controller: camera,
                            onDetect: _onDetect,
                            errorBuilder: (context, error) => ColoredBox(
                              color: Colors.black,
                              child: Center(
                                child: Padding(
                                  padding: const EdgeInsets.all(24),
                                  child: Text(
                                    tr.cameraUnavailable,
                                    textAlign: TextAlign.center,
                                    style: const TextStyle(color: Colors.white),
                                  ),
                                ),
                              ),
                            ),
                          ),
                          const _Frame(),
                        ],
                      ),
                    ),
                  ),
                  const SizedBox(height: 12),
                  Text(tr.scanHint, textAlign: TextAlign.center, style: theme.textTheme.bodyMedium),
                  const SizedBox(height: 24),
                ] else ...[
                  const Center(child: BrandMark(size: 96)),
                  const SizedBox(height: 16),
                  Text(
                    tr.desktopScanHint,
                    textAlign: TextAlign.center,
                    style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                  ),
                  const SizedBox(height: 24),
                ],
                TextField(
                  controller: _link,
                  enabled: !_busy,
                  decoration: InputDecoration(
                    labelText: tr.pasteLink,
                    hintText: tr.linkFieldHint,
                    errorText: _error,
                    border: const OutlineInputBorder(),
                    suffixIcon: IconButton(
                      tooltip: tr.paste,
                      icon: const Icon(Icons.content_paste_rounded),
                      onPressed: _busy ? null : _paste,
                    ),
                  ),
                  onSubmitted: (v) => _use(v.trim()),
                ),
                const SizedBox(height: 12),
                FilledButton(
                  onPressed: _busy ? null : () => _use(_link.text.trim()),
                  child: Text(tr.connect),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// Рамка-прицел поверх изображения камеры.
class _Frame extends StatelessWidget {
  const _Frame();

  @override
  Widget build(BuildContext context) {
    return IgnorePointer(
      child: Center(
        child: FractionallySizedBox(
          widthFactor: 0.7,
          heightFactor: 0.7,
          child: DecoratedBox(
            decoration: BoxDecoration(
              border: Border.all(color: Colors.white.withValues(alpha: 0.85), width: 3),
              borderRadius: BorderRadius.circular(16),
            ),
          ),
        ),
      ),
    );
  }
}
