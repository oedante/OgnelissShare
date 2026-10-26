import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:intl/date_symbol_data_local.dart';
import 'package:ogneliss_share/app/format.dart';
import 'package:ogneliss_share/core/models.dart';
import 'package:ogneliss_share/l10n/l10n.dart';

void main() {
  setUpAll(() async {
    await initializeDateFormatting('ru');
    await initializeDateFormatting('en');
  });

  group('русский', () {
    setUp(() => setTr(lookupAppLocalizations(const Locale('ru'))));

    test('размеры', () {
      expect(formatBytes(0), '0 Б');
      expect(formatBytes(1536), '1,5 КБ');
      expect(formatBytes(150 * 1024 * 1024), '150 МБ');
      expect(formatRate(1536), '1,5 КБ/с');
    });

    test('множественное число', () {
      expect(tr.files(1), '1 файл');
      expect(tr.files(3), '3 файла');
      expect(tr.files(11), '11 файлов');
      expect(tr.files(21), '21 файл');
      expect(tr.files(112), '112 файлов');
    });

    test('имена передачи', () {
      expect(formatNames(['отчёт.pdf'], 1), 'отчёт.pdf');
      expect(formatNames(['a.jpg', 'b.jpg', 'c.jpg'], 3), 'a.jpg, b.jpg и ещё 1');
      expect(formatNames(['Фото'], 120), 'Фото · 120 файлов');
      expect(formatNames([], 2), '2 файла');
    });

    test('время', () {
      final now = DateTime(2026, 10, 26, 12);
      int ago(Duration d) => now.subtract(d).millisecondsSinceEpoch ~/ 1000;
      expect(formatAgo(ago(const Duration(seconds: 10)), now: now), 'только что');
      expect(formatAgo(ago(const Duration(minutes: 5)), now: now), '5 мин назад');
      expect(formatAgo(ago(const Duration(hours: 3)), now: now), '3 ч назад');
      expect(formatAgo(ago(const Duration(hours: 20)), now: now), 'вчера');
      expect(formatAgo(ago(const Duration(days: 5)), now: now), '21.10.2026');
      expect(formatClock(299), '4:59');
      expect(formatDuration(90), '2 мин');
      expect(formatDuration(3900), '1 ч 5 мин');
      expect(formatLeft(0, 100, 0), isNull);
    });

    test('ошибки и пустые имена', () {
      expect(tr.error('unreachable'), contains('недоступно'));
      expect(tr.error('что-то новое'), 'Не получилось.');
      expect(tr.deviceName(''), 'Без имени');
      expect(tr.deviceName('Ноутбук'), 'Ноутбук');
    });
  });

  test('english', () {
    setTr(lookupAppLocalizations(const Locale('en')));
    expect(formatBytes(1536), '1.5 KB');
    expect(tr.files(1), '1 file');
    expect(tr.files(2), '2 files');
    expect(formatNames(['a.jpg', 'b.jpg', 'c.jpg'], 3), 'a.jpg, b.jpg and 1 more');
    expect(tr.error('no_space'), contains('space'));
  });

  test('язык по системе', () {
    expect(appLocale(const [Locale('ru', 'RU')]), const Locale('ru'));
    expect(appLocale(const [Locale('uk')]), const Locale('ru'));
    expect(appLocale(const [Locale('de'), Locale('ru')]), const Locale('ru'));
    expect(appLocale(const [Locale('en', 'US'), Locale('ru')]), const Locale('en'));
    expect(appLocale(const [Locale('fr')]), const Locale('en'));
    expect(appLocale(null), const Locale('en'));
  });

  test('снимок состояния разбирается', () {
    final snap = Snapshot.fromJson({
      'me': {'id': 'abc', 'code': 'AB12', 'name': 'Ноутбук', 'platform': 'linux', 'online': true},
      'peers': [
        {'id': 'p1', 'name': 'Телефон', 'platform': 'android', 'kind': 'paired', 'online': true},
        {'id': 'p2', 'name': 'Гость', 'platform': 'ios', 'kind': 'session', 'online': false},
      ],
      'invite': null,
      'requests': [
        {'id': 'r1', 'kind': 'pair', 'peer': 'p3', 'name': 'Планшет', 'code': '1234'},
      ],
      'transfers': [
        {
          'id': 't1',
          'dir': 'in',
          'kind': 'files',
          'total': 100,
          'done': 50,
          'status': 'active',
          'names': ['a'],
          'saved_is_dir': true,
        },
      ],
    });
    expect(snap.me.name, 'Ноутбук');
    expect(snap.peers.length, 2);
    expect(snap.peers[1].isSession, isTrue);
    expect(snap.peer('p1')?.name, 'Телефон');
    expect(snap.invite, isNull);
    expect(snap.requests.single.code, '1234');
    final t = snap.transfers.single;
    expect(t.progress, 0.5);
    expect(t.outgoing, isFalse);
    expect(t.isActive, isTrue);
    expect(t.savedIsDir, isTrue);
  });
}
