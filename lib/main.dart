// Ogneliss Share — прямая передача файлов между своими устройствами
// без учётных записей. Интерфейс на Flutter, ядро на Rust (каталог rust/)
// в отдельном изоляте.

import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'app/app_model.dart';
import 'app/theme.dart';
import 'l10n/l10n.dart';
import 'platform/startup.dart';
import 'ui/home_screen.dart';
import 'ui/shell.dart';
import 'ui/widgets.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  LicenseRegistry.addLicense(_rustLicenses);
  // Первый кадр — сразу; ядро запускается в фоне (см. _AppState._boot).
  runApp(const OgnelissShareApp());
}

/// Лицензии библиотек Rust из ядра (их собирает tool/rust_licenses.py) —
/// на экране «Лицензии» рядом с пакетами Dart. Разбор — в фоновом изоляте.
Stream<LicenseEntry> _rustLicenses() async* {
  final raw = await rootBundle.loadString('assets/licenses/rust.json');
  final entries = await compute(_parseLicenses, raw);
  for (final (packages, text) in entries) {
    yield LicenseEntryWithLineBreaks(packages, text);
  }
}

List<(List<String>, String)> _parseLicenses(String raw) {
  final list = jsonDecode(raw);
  if (list is! List) return const [];
  final out = <(List<String>, String)>[];
  for (final e in list) {
    if (e is! Map) continue;
    final packages = [for (final p in (e['packages'] as List? ?? const [])) if (p is String) p];
    final text = e['text'];
    if (packages.isEmpty || text is! String || text.isEmpty) continue;
    out.add((packages, text));
  }
  return out;
}

class OgnelissShareApp extends StatefulWidget {
  const OgnelissShareApp({super.key});

  @override
  State<OgnelissShareApp> createState() => _AppState();
}

class _AppState extends State<OgnelissShareApp> {
  AppModel? _model;
  String? _error;

  @override
  void initState() {
    super.initState();
    _boot();
  }

  Future<void> _boot() async {
    try {
      final model = await startApp();
      if (!mounted) return;
      setState(() => _model = model);
    } catch (e, st) {
      if (!mounted) return;
      setState(() => _error = '$e\n\n$st');
    }
  }

  @override
  Widget build(BuildContext context) {
    final model = _model;
    final Widget home;
    if (model != null) {
      home = const Shell(child: HomeScreen());
    } else if (_error != null) {
      home = _StartupError(error: _error!);
    } else {
      home = const _Splash();
    }
    return AppScope(
      model: model,
      child: MaterialApp(
        onGenerateTitle: (context) => AppLocalizations.of(context).appName,
        debugShowCheckedModeBanner: false,
        theme: AppTheme.light(),
        darkTheme: AppTheme.dark(),
        navigatorKey: navigatorKey,
        scaffoldMessengerKey: messengerKey,
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        localeListResolutionCallback: (locales, supported) => appLocale(locales),
        builder: (context, child) {
          // Тексты для мест без BuildContext (уведомления, форматирование).
          setTr(AppLocalizations.of(context));
          return child ?? const SizedBox.shrink();
        },
        home: home,
      ),
    );
  }
}

/// Пока ядро запускается (обычно доли секунды).
class _Splash extends StatelessWidget {
  const _Splash();

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      body: Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const BrandMark(size: 112),
            const SizedBox(height: 24),
            const SizedBox(width: 28, height: 28, child: CircularProgressIndicator(strokeWidth: 3)),
            const SizedBox(height: 16),
            Text(tr.starting, style: theme.textTheme.bodyMedium),
          ],
        ),
      ),
    );
  }
}

/// Ядро не запустилось (например, библиотеки нет в сборке) — показываем
/// причину, чтобы её можно было скопировать в сообщение об ошибке.
class _StartupError extends StatelessWidget {
  const _StartupError({required this.error});

  final String error;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(24),
          children: [
            const SizedBox(height: 24),
            const Center(child: BrandMark(size: 96)),
            const SizedBox(height: 16),
            Text(tr.startupError, textAlign: TextAlign.center, style: theme.textTheme.titleLarge),
            const SizedBox(height: 16),
            SelectableText(error, style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
          ],
        ),
      ),
    );
  }
}
