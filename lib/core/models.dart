// Снимок состояния ядра (команда snapshot) в виде классов Dart.

String _str(Object? v) => v is String ? v : '';
String? _strOrNull(Object? v) => v is String ? v : null;
int _int(Object? v) => v is num ? v.toInt() : 0;
bool _bool(Object? v) => v == true;
Map<String, dynamic> _map(Object? v) => v is Map ? Map<String, dynamic>.from(v) : <String, dynamic>{};
List<Map<String, dynamic>> _maps(Object? v) =>
    v is List ? [for (final e in v) if (e is Map) Map<String, dynamic>.from(e)] : const [];
List<String> _strings(Object? v) => v is List ? [for (final e in v) if (e is String) e] : const [];

/// Это устройство.
class Me {
  const Me({
    required this.id,
    required this.code,
    required this.name,
    required this.platform,
    required this.online,
    required this.relay,
    required this.downloadDir,
    required this.downloadDirCustom,
    required this.version,
  });

  factory Me.fromJson(Map<String, dynamic> j) => Me(
        id: _str(j['id']),
        code: _str(j['code']),
        name: _str(j['name']),
        platform: _str(j['platform']),
        online: _bool(j['online']),
        relay: _str(j['relay']),
        downloadDir: _str(j['download_dir']),
        downloadDirCustom: _bool(j['download_dir_custom']),
        version: _str(j['version']),
      );

  static const empty = Me(
    id: '',
    code: '',
    name: '',
    platform: '',
    online: false,
    relay: '',
    downloadDir: '',
    downloadDirCustom: false,
    version: '',
  );

  final String id;

  /// Четыре знака из отпечатка ключа — чтобы отличать одинаковые имена.
  final String code;
  final String name;
  final String platform;

  /// Есть связь с ретранслятором (интернет). Без неё работает локальная сеть.
  final bool online;
  final String relay;
  final String downloadDir;
  final bool downloadDirCustom;
  final String version;
}

/// Связанное устройство («Мои устройства») или разовый собеседник.
class Peer {
  const Peer({
    required this.id,
    required this.code,
    required this.name,
    required this.platform,
    required this.kind,
    required this.online,
    required this.connecting,
    required this.lastSeen,
  });

  factory Peer.fromJson(Map<String, dynamic> j) => Peer(
        id: _str(j['id']),
        code: _str(j['code']),
        name: _str(j['name']),
        platform: _str(j['platform']),
        kind: _str(j['kind']),
        online: _bool(j['online']),
        connecting: _bool(j['connecting']),
        lastSeen: _int(j['last_seen']),
      );

  final String id;
  final String code;
  final String name;
  final String platform;

  /// paired — постоянная связь; session — разовый обмен.
  final String kind;
  final bool online;
  final bool connecting;

  /// Последний раз на связи (секунды Unix).
  final int lastSeen;

  bool get isSession => kind == 'session';
}

/// Кто воспользовался показанным QR-кодом.
class Joined {
  const Joined({required this.name, required this.platform, required this.state});

  factory Joined.fromJson(Map<String, dynamic> j) =>
      Joined(name: _str(j['name']), platform: _str(j['platform']), state: _str(j['state']));

  final String name;
  final String platform;

  /// pending — ждём подтверждения; done — связаны; declined — отклонено.
  final String state;
}

/// Показанный QR-код.
class Invite {
  const Invite({required this.mode, required this.uri, required this.expiresIn, this.joined});

  factory Invite.fromJson(Map<String, dynamic> j) => Invite(
        mode: _str(j['mode']),
        uri: _str(j['uri']),
        expiresIn: _int(j['expires_in']),
        joined: j['joined'] is Map ? Joined.fromJson(_map(j['joined'])) : null,
      );

  /// pair — «Моё устройство», once — разовый обмен.
  final String mode;
  final String uri;
  final int expiresIn;
  final Joined? joined;
}

/// Вопрос к пользователю: подключить устройство или принять файлы.
class Request {
  const Request({
    required this.id,
    required this.kind,
    required this.peer,
    required this.name,
    required this.platform,
    required this.code,
    required this.count,
    required this.total,
    required this.names,
  });

  factory Request.fromJson(Map<String, dynamic> j) => Request(
        id: _str(j['id']),
        kind: _str(j['kind']),
        peer: _str(j['peer']),
        name: _str(j['name']),
        platform: _str(j['platform']),
        code: _str(j['code']),
        count: _int(j['count']),
        total: _int(j['total']),
        names: _strings(j['names']),
      );

  final String id;

  /// pair — подключить устройство; offer — принять файлы.
  final String kind;
  final String peer;
  final String name;
  final String platform;

  /// Проверочный код сопряжения (для pair).
  final String code;
  final int count;
  final int total;
  final List<String> names;
}

/// Передача файлов или текста.
class TransferItem {
  const TransferItem({
    required this.id,
    required this.peer,
    required this.peerName,
    required this.outgoing,
    required this.isText,
    required this.names,
    required this.count,
    required this.total,
    required this.done,
    required this.rate,
    required this.status,
    required this.error,
    required this.byPeer,
    required this.text,
    required this.textTruncated,
    required this.savedTo,
    required this.failedFiles,
    required this.started,
    required this.finished,
    required this.canRetry,
  });

  factory TransferItem.fromJson(Map<String, dynamic> j) => TransferItem(
        id: _str(j['id']),
        peer: _str(j['peer']),
        peerName: _str(j['peer_name']),
        outgoing: j['dir'] == 'out',
        isText: j['kind'] == 'text',
        names: _strings(j['names']),
        count: _int(j['count']),
        total: _int(j['total']),
        done: _int(j['done']),
        rate: _int(j['rate']),
        status: _str(j['status']),
        error: _strOrNull(j['error']),
        byPeer: _bool(j['by_peer']),
        text: _strOrNull(j['text']),
        textTruncated: _bool(j['text_truncated']),
        savedTo: _strOrNull(j['saved_to']),
        failedFiles: _int(j['failed_files']),
        started: _int(j['started']),
        finished: _int(j['finished']),
        canRetry: _bool(j['can_retry']),
      );

  final String id;
  final String peer;
  final String peerName;
  final bool outgoing;
  final bool isText;
  final List<String> names;
  final int count;
  final int total;
  final int done;

  /// Скорость, байт в секунду.
  final int rate;

  /// preparing, connecting, waiting_peer, waiting, asking, active, retrying,
  /// done, failed, declined, cancelled.
  final String status;
  final String? error;
  final bool byPeer;
  final String? text;
  final bool textTruncated;
  final String? savedTo;
  final int failedFiles;
  final int started;
  final int finished;
  final bool canRetry;

  static const _final = {'done', 'failed', 'declined', 'cancelled'};

  bool get isFinished => _final.contains(status);
  bool get isActive => !isFinished;
  bool get isDone => status == 'done';

  /// Доля переданного от 0 до 1.
  double get progress {
    if (total <= 0) return isDone ? 1 : 0;
    return (done / total).clamp(0.0, 1.0).toDouble();
  }
}

class Snapshot {
  const Snapshot({
    required this.me,
    required this.peers,
    required this.invite,
    required this.requests,
    required this.transfers,
  });

  factory Snapshot.fromJson(Map<String, dynamic> j) => Snapshot(
        me: Me.fromJson(_map(j['me'])),
        peers: [for (final p in _maps(j['peers'])) Peer.fromJson(p)],
        invite: j['invite'] is Map ? Invite.fromJson(_map(j['invite'])) : null,
        requests: [for (final r in _maps(j['requests'])) Request.fromJson(r)],
        transfers: [for (final t in _maps(j['transfers'])) TransferItem.fromJson(t)],
      );

  static const empty = Snapshot(me: Me.empty, peers: [], invite: null, requests: [], transfers: []);

  final Me me;
  final List<Peer> peers;
  final Invite? invite;
  final List<Request> requests;

  /// Сначала новые.
  final List<TransferItem> transfers;

  Peer? peer(String id) {
    for (final p in peers) {
      if (p.id == id) return p;
    }
    return null;
  }
}

/// Разовое событие ядра (из osh_poll).
class CoreEvent {
  const CoreEvent(this.data);

  final Map<String, dynamic> data;

  String get type => _str(data['type']);
  String get name => _str(data['name']);
  String get peer => _str(data['peer']);
  String str(String key) => _str(data[key]);
  int number(String key) => _int(data[key]);
  bool flag(String key) => _bool(data[key]);
  String? maybe(String key) => _strOrNull(data[key]);
}
