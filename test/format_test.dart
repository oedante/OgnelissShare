import 'package:flutter_test/flutter_test.dart';
import 'package:ogneliss_share/app/format.dart';
import 'package:ogneliss_share/app/strings.dart';
import 'package:ogneliss_share/core/models.dart';

void main() {
  group('русский', () {
    setUp(() => S.current = const SRu());

    test('размеры', () {
      expect(formatBytes(0), '0 Б');
      expect(formatBytes(1536), '1,5 КБ');
      expect(formatBytes(150 * 1024 * 1024), '150 МБ');
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
      expect(formatClock(299), '4:59');
      expect(formatDuration(90), '2 мин');
      expect(formatLeft(0, 100, 0), isNull);
    });
  });

  test('english', () {
    S.current = const SEn();
    expect(formatBytes(1536), '1.5 KB');
    expect(tr.files(1), '1 file');
    expect(tr.files(2), '2 files');
    S.current = const SRu();
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
        {'id': 't1', 'dir': 'out', 'kind': 'files', 'total': 100, 'done': 50, 'status': 'active', 'names': ['a']},
      ],
    });
    expect(snap.me.name, 'Ноутбук');
    expect(snap.peers.length, 2);
    expect(snap.peers[1].isSession, isTrue);
    expect(snap.peer('p1')?.name, 'Телефон');
    expect(snap.invite, isNull);
    expect(snap.requests.single.code, '1234');
    expect(snap.transfers.single.progress, 0.5);
    expect(snap.transfers.single.outgoing, isTrue);
    expect(snap.transfers.single.isActive, isTrue);
  });
}
