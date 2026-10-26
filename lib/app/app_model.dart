// Состояние приложения для интерфейса: снимок ядра и действия.
//
// Ядро опрашивается несколько раз в секунду (osh_poll — дешёвый вызов):
// изменился номер версии — берём новый снимок и перерисовываем экран.
// Разовые события (получены файлы, устройство отключилось) уходят в поток
// [events], их показывает Shell.

import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import '../core/models.dart';
import '../core/native.dart';

class AppModel extends ChangeNotifier {
  AppModel(this.native);

  final Native native;

  Snapshot snapshot = Snapshot.empty;
  int _version = -1;
  Timer? _timer;
  bool _active = true;
  AppLifecycleListener? _lifecycle;
  final _events = StreamController<CoreEvent>.broadcast();

  Stream<CoreEvent> get events => _events.stream;

  /// Итог последнего подключения по коду — на случай, если экран
  /// подписался на события позже, чем итог пришёл.
  CoreEvent? lastJoin;

  static bool get _mobile => Platform.isAndroid || Platform.isIOS;

  void start() {
    _reload();
    _schedule();
    _lifecycle = AppLifecycleListener(onStateChange: _onLifecycle);
  }

  @override
  void dispose() {
    _timer?.cancel();
    _lifecycle?.dispose();
    _events.close();
    super.dispose();
  }

  void _schedule() {
    _timer?.cancel();
    _timer = Timer.periodic(Duration(milliseconds: _active ? 200 : 2000), (_) => _tick());
  }

  void _onLifecycle(AppLifecycleState state) {
    // На телефоне в фоне не тратим заряд; на компьютере приложение работает
    // и свёрнутым — принимает файлы.
    final active = _mobile ? state == AppLifecycleState.resumed : state != AppLifecycleState.detached;
    if (active == _active) return;
    _active = active;
    _quiet(() => native.call('set_active', {'active': active}));
    _schedule();
    if (active) _tick();
  }

  void _tick() {
    final Map<String, dynamic> polled;
    try {
      polled = native.poll();
    } catch (_) {
      return;
    }
    final list = polled['events'];
    if (list is List) {
      for (final e in list) {
        if (e is! Map) continue;
        final event = CoreEvent(Map<String, dynamic>.from(e));
        if (event.type == 'join_result') lastJoin = event;
        _events.add(event);
      }
    }
    final v = polled['v'];
    final version = v is num ? v.toInt() : 0;
    if (version != _version) {
      _version = version;
      _reload();
    }
  }

  void _reload() {
    try {
      final r = native.call('snapshot');
      if (r is Map) {
        snapshot = Snapshot.fromJson(Map<String, dynamic>.from(r));
        notifyListeners();
      }
    } catch (_) {
      // Снимок повторится на следующем такте.
    }
  }

  void _quiet(void Function() f) {
    try {
      f();
    } catch (_) {}
  }

  /// Вызвать команду и сразу обновить состояние.
  dynamic _act(String cmd, [Map<String, Object?> args = const {}]) {
    final r = native.call(cmd, args);
    _tick();
    return r;
  }

  // ------------------------------------------------------------ действия

  void refreshNow() => _quiet(() => _act('refresh'));

  void setName(String name) => _act('set_name', {'name': name});

  void setDownloadDir(String? path) => _act('set_download_dir', {'path': path ?? ''});

  /// Показать QR-код: mode = 'pair' (моё устройство) или 'once' (разовый обмен).
  Invite? createInvite(String mode) {
    final r = _act('create_invite', {'mode': mode});
    return r is Map ? Invite.fromJson(Map<String, dynamic>.from(r)) : null;
  }

  void cancelInvite() => _quiet(() => _act('cancel_invite'));

  /// Подключиться по отсканированному коду. Итог придёт событием join_result.
  JoinInfo useInvite(String uri) {
    lastJoin = null;
    final r = _act('use_invite', {'uri': uri.trim()});
    final m = r is Map ? Map<String, dynamic>.from(r) : <String, dynamic>{};
    return JoinInfo(
      peer: (m['peer'] as String?) ?? '',
      name: (m['name'] as String?) ?? '',
      platform: (m['platform'] as String?) ?? '',
      mode: (m['mode'] as String?) ?? 'pair',
      code: (m['code'] as String?) ?? '',
    );
  }

  void respondPair(String id, bool accept) => _quiet(() => _act('respond_pair', {'id': id, 'accept': accept}));

  void removePeer(String peer) => _act('remove_peer', {'peer': peer});

  String sendFiles(String peer, List<String> paths) =>
      _act('send_files', {'peer': peer, 'paths': paths}) as String;

  String sendText(String peer, String text) => _act('send_text', {'peer': peer, 'text': text}) as String;

  void respondOffer(String transfer, bool accept) =>
      _quiet(() => _act('respond_offer', {'id': transfer, 'accept': accept}));

  void cancelTransfer(String id) => _quiet(() => _act('cancel_transfer', {'id': id}));

  void retryTransfer(String id) => _act('retry_transfer', {'id': id});

  void dismissTransfer(String id) => _quiet(() => _act('dismiss_transfer', {'id': id}));

  void clearTransfers() => _quiet(() => _act('clear_transfers'));

  /// Текст передачи целиком (в снимке — только начало).
  String? fullText(String id) {
    try {
      final r = native.call('get_text', {'id': id});
      return r is String ? r : null;
    } catch (_) {
      return null;
    }
  }

  List<String> logs() {
    try {
      final r = native.call('logs');
      return r is List ? [for (final l in r) if (l is String) l] : const [];
    } catch (_) {
      return const [];
    }
  }
}

/// Что известно о другом устройстве сразу после сканирования его кода.
class JoinInfo {
  const JoinInfo({
    required this.peer,
    required this.name,
    required this.platform,
    required this.mode,
    required this.code,
  });

  final String peer;
  final String name;
  final String platform;
  final String mode;

  /// Четыре цифры, которые должны совпасть на обоих экранах.
  final String code;

  bool get once => mode == 'once';
}

/// Доступ к [AppModel] из любого виджета: `AppScope.of(context)`.
class AppScope extends InheritedNotifier<AppModel> {
  const AppScope({super.key, required AppModel model, required super.child}) : super(notifier: model);

  static AppModel of(BuildContext context) {
    final scope = context.dependOnInheritedWidgetOfExactType<AppScope>();
    assert(scope != null, 'AppScope не найден');
    return scope!.notifier!;
  }

  /// Без подписки на изменения — для обработчиков нажатий.
  static AppModel read(BuildContext context) {
    final scope = context.getInheritedWidgetOfExactType<AppScope>();
    return scope!.notifier!;
  }
}
