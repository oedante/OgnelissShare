import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:ogneliss_share/app/strings.dart';
import 'package:ogneliss_share/app/theme.dart';
import 'package:ogneliss_share/ui/shell.dart';
import 'package:ogneliss_share/ui/widgets.dart';

void main() {
  testWidgets('окно с полученным текстом', (tester) async {
    S.current = const SRu();
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.light(),
        home: const Scaffold(body: TextDialog(title: 'Текст от «Телефон»', text: 'Привет')),
      ),
    );
    expect(find.text('Привет'), findsOneWidget);
    expect(find.text('Копировать'), findsOneWidget);
  });

  testWidgets('значок устройства', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: AppTheme.dark(),
        home: const Scaffold(
          body: Row(children: [PeerAvatar(platform: 'android', online: true), CodeChip('AB12'), Tag('разовый')]),
        ),
      ),
    );
    expect(find.byIcon(Icons.phone_android), findsOneWidget);
    expect(find.text('AB12'), findsOneWidget);
  });
}
