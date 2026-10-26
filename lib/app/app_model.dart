// Состояние приложения для интерфейса: снимок ядра и действия.
//
// Ядро работает в своём изоляте (CoreClient): оно само присылает новые
// снимки состояния и разовые события, а команды возвращают Future —
// поток интерфейса ничего не ждёт.

import 'dart:async';
import 'dart:io';

import 'package:flutter/widgets.dart';

import '../core/core_client.dart';
import '../core/models.dart';

class AppModel extends ChangeNotifier {
  AppModel(this.core) {
    _subs
      ..add(core.snapshots.listen(_onSnapshot))
      ..add(core.events.listen(_onEvent));
    _lifecycle = AppLifecycleListener(onStateChange: _onLifecycle);
  }

  final CoreClient core;

  Snapshot snapshot = Snapshot.empty;
  final _events = StreamController<CoreEvent>.broadcast();
  final _subs = <StreamSubscription<Object?>>[];
  AppLifecycleListener? _lifecycle;
  bool _active = true;

  /// Разовые события ядра: получены файлы, пришёл текст, связь разорвана…
  Stream<CoreEvent> get events => _events.stream;

  /// Итог последнего подключения по коду — на случай, если экран
  /// подписался на события позже, чем итог пришёл.
  CoreEvent? lastJoin;

  static bool get _mobile => Platform.isAndroid || Platform.isIOS;

  @override
  void dispose() {
    for (final s in _subs) {
      s.cancel();
    }
    _lifecycle?.dispose();
    _events.close();
    super.dispose();
  }

  void _onSnapshot(Snapshot s) {
    snapshot = s;
    notifyListeners();
  }

  void _onEvent(CoreEvent e) {
    if (e.type == 'join_result') lastJoin = e;
    _events.add(e);
  }

  void _onLifecycle(AppLifecycleState state) {
    // На телефоне в фоне не тратим заряд; на компьютере приложение работает
    // и свёрнутым — принимает файлы.
    final active = _mobile ? state == AppLifecycleState.resumed : state != AppLifecycleState.detached;
    if (active == _active) return;
    _active = active;
    core.setActive(active);
    _quiet(core.call('set_active', {'active': active}));
  }

  /// Команда без ответа: ошибки не важны (например, вопрос уже снят).
  Future<void> _quiet(Future<dynamic> f) async {
    try {
      await f;
    } catch (_) {}
  }

  // ------------------------------------------------------------ действия

  Future<void> refreshNow() => _quiet(core.call('refresh'));

  Future<void> setName(String name) => core.call('set_name', {'name': name});

  /// Своя папка для полученных файлов; null — папка по умолчанию.
  Future<void> setDownloadDir(String? path) => core.call('set_download_dir', {'path': path ?? ''});

  /// Показать QR-код: mode = 'pair' (моё устройство) или 'once' (разовый обмен).
  Future<Invite?> createInvite(String mode) async {
    final r = await core.call('create_invite', {'mode': mode});
    return r is Map ? Invite.fromJson(Map<String, dynamic>.from(r)) : null;
  }

  Future<void> cancelInvite() => _quiet(core.call('cancel_invite'));

  /// Подключиться по отсканированному коду. Итог придёт событием join_result.
  Future<JoinInfo> useInvite(String uri) async {
    lastJoin = null;
    final r = await core.call('use_invite', {'uri': uri.trim()});
    final m = r is Map ? Map<String, dynamic>.from(r) : <String, dynamic>{};
    return JoinInfo(
      peer: (m['peer'] as String?) ?? '',
      name: (m['name'] as String?) ?? '',
      platform: (m['platform'] as String?) ?? '',
      mode: (m['mode'] as String?) ?? 'pair',
      code: (m['code'] as String?) ?? '',
    );
  }

  Future<void> respondPair(String id, bool accept) =>
      _quiet(core.call('respond_pair', {'id': id, 'accept': accept}));

  Future<void> removePeer(String peer) => core.call('remove_peer', {'peer': peer});

  Future<String> sendFiles(String peer, List<String> paths) async =>
      (await core.call('send_files', {'peer': peer, 'paths': paths})) as String;

  Future<String> sendText(String peer, String text) async =>
      (await core.call('send_text', {'peer': peer, 'text': text})) as String;

  Future<void> respondOffer(String transfer, bool accept) =>
      _quiet(core.call('respond_offer', {'id': transfer, 'accept': accept}));

  Future<void> cancelTransfer(String id) => _quiet(core.call('cancel_transfer', {'id': id}));

  Future<void> retryTransfer(String id) => core.call('retry_transfer', {'id': id});

  Future<void> dismissTransfer(String id) => _quiet(core.call('dismiss_transfer', {'id': id}));

  Future<void> clearTransfers() => _quiet(core.call('clear_transfers'));

  /// Текст передачи целиком (в снимке — только начало).
  Future<String?> fullText(String id) async {
    try {
      final r = await core.call('get_text', {'id': id});
      return r is String ? r : null;
    } catch (_) {
      return null;
    }
  }

  Future<List<String>> logs() async {
    try {
      final r = await core.call('logs');
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
/// Пока ядро запускается, модели ещё нет (notifier == null).
class AppScope extends InheritedNotifier<AppModel> {
  const AppScope({super.key, required AppModel? model, required super.child}) : super(notifier: model);

  static AppModel of(BuildContext context) {
    final scope = context.dependOnInheritedWidgetOfExactType<AppScope>();
    assert(scope?.notifier != null, 'AppScope: модели ещё нет');
    return scope!.notifier!;
  }

  /// Без подписки на изменения — для обработчиков нажатий.
  static AppModel read(BuildContext context) {
    final scope = context.getInheritedWidgetOfExactType<AppScope>();
    return scope!.notifier!;
  }
}
