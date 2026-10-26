import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ogneliss_share/app/theme.dart';
import 'package:ogneliss_share/l10n/l10n.dart';
import 'package:ogneliss_share/ui/shell.dart';
import 'package:ogneliss_share/ui/widgets.dart';

Widget _app(Widget home, {Locale locale = const Locale('ru')}) => MaterialApp(
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      locale: locale,
      supportedLocales: AppLocalizations.supportedLocales,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      builder: (context, child) {
        setTr(AppLocalizations.of(context));
        return child!;
      },
      home: home,
    );

void main() {
  testWidgets('окно с полученным текстом (ru)', (tester) async {
    await tester.pumpWidget(_app(const Scaffold(body: TextDialog(title: 'Текст от «Телефон»', text: 'Привет'))));
    await tester.pumpAndSettle();
    expect(find.text('Привет'), findsOneWidget);
    expect(find.text('Копировать'), findsOneWidget);
  });

  testWidgets('окно с полученным текстом (en)', (tester) async {
    await tester.pumpWidget(
      _app(const Scaffold(body: TextDialog(title: 'Text', text: 'Hello')), locale: const Locale('en')),
    );
    await tester.pumpAndSettle();
    expect(find.text('Copy'), findsOneWidget);
  });

  testWidgets('значок устройства', (tester) async {
    await tester.pumpWidget(
      _app(
        const Scaffold(
          body: Row(children: [PeerAvatar(platform: 'android', online: true), CodeChip('AB12'), Tag('разовый')]),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byIcon(Icons.phone_android), findsOneWidget);
    expect(find.text('AB12'), findsOneWidget);
  });
}
