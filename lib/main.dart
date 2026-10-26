// Ogneliss Share — прямая передача файлов между своими устройствами
// без учётных записей. Интерфейс на Flutter, ядро на Rust (каталог rust/).

import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_localizations/flutter_localizations.dart';

import 'app/app_model.dart';
import 'app/strings.dart';
import 'app/theme.dart';
import 'platform/startup.dart';
import 'ui/home_screen.dart';
import 'ui/shell.dart';
import 'ui/widgets.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  S.load(WidgetsBinding.instance.platformDispatcher.locale);
  _registerRustLicenses();
  final start = await startCore();
  runApp(OgnelissShareApp(start: start));
}

/// Лицензии библиотек Rust из ядра (их собирает tool/rust_licenses.py) —
/// на экране «Лицензии» рядом с пакетами Dart.
void _registerRustLicenses() {
  LicenseRegistry.addLicense(() async* {
    final raw = await rootBundle.loadString('assets/licenses/rust.json');
    final list = jsonDecode(raw);
    if (list is! List) return;
    for (final e in list) {
      if (e is! Map) continue;
      final packages = [for (final p in (e['packages'] as List? ?? const [])) if (p is String) p];
      final text = e['text'];
      if (packages.isEmpty || text is! String || text.isEmpty) continue;
      yield LicenseEntryWithLineBreaks(packages, text);
    }
  });
}

class OgnelissShareApp extends StatelessWidget {
  const OgnelissShareApp({super.key, required this.start});

  final CoreStart start;

  @override
  Widget build(BuildContext context) {
    final model = start.model;
    final app = MaterialApp(
      title: tr.appName,
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      navigatorKey: navigatorKey,
      scaffoldMessengerKey: messengerKey,
      locale: S.locale,
      supportedLocales: const [Locale('ru'), Locale('en')],
      localizationsDelegates: GlobalMaterialLocalizations.delegates,
      home: model == null ? _StartupError(error: start.error ?? '') : const Shell(child: HomeScreen()),
    );
    return model == null ? app : AppScope(model: model, child: app);
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
