// Ядро — в отдельном изоляте: все вызовы FFI, опрос и разбор состояния идут
// там, а поток интерфейса получает готовые объекты и ни одного вызова не
// ждёт. Команды возвращают Future.
//
// Изолят → интерфейс:
//   ['ready', SendPort]                 — ядро запущено, сюда слать команды;
//   ['fatal', String]                   — ядро не запустилось;
//   ['reply', id, ok, result|code, detail];
//   ['event', Map]                      — разовое событие ядра;
//   ['snapshot', Snapshot]              — новое состояние.
// Интерфейс → изолят:
//   [id, cmd, args]                     — команда;
//   ['active', bool]                    — приложение на экране / в фоне.

import 'dart:async';
import 'dart:isolate';

import 'models.dart';
import 'native.dart';

/// Как часто опрашивать ядро, когда приложение на экране и в фоне.
const _activePoll = Duration(milliseconds: 150);
const _backgroundPoll = Duration(seconds: 2);

class CoreClient {
  CoreClient._();

  SendPort? _commands;
  int _nextId = 1;
  final _replies = <int, Completer<dynamic>>{};
  final _snapshots = StreamController<Snapshot>.broadcast();
  final _events = StreamController<CoreEvent>.broadcast();

  Stream<Snapshot> get snapshots => _snapshots.stream;
  Stream<CoreEvent> get events => _events.stream;

  /// Запустить изолят и в нём ядро (команда init с [init]).
  static Future<CoreClient> start(Map<String, Object?> init) async {
    final client = CoreClient._();
    final ready = Completer<void>();
    final inbox = ReceivePort('ogneliss-core');
    inbox.listen((message) => client._onMessage(message, ready));
    final errors = ReceivePort();
    errors.listen((error) {
      if (!ready.isCompleted) ready.completeError(CoreError('failed', '$error'));
    });
    try {
      await Isolate.spawn<List<Object?>>(
        _workerMain,
        <Object?>[inbox.sendPort, init],
        debugName: 'ogneliss-core',
        onError: errors.sendPort,
        errorsAreFatal: false,
      );
      await ready.future;
    } catch (_) {
      inbox.close();
      errors.close();
      rethrow;
    }
    return client;
  }

  void _onMessage(Object? message, Completer<void> ready) {
    if (message is! List || message.isEmpty) return;
    switch (message[0]) {
      case 'ready':
        _commands = message[1] as SendPort;
        if (!ready.isCompleted) ready.complete();
      case 'fatal':
        if (!ready.isCompleted) ready.completeError(CoreError('failed', message[1] as String));
      case 'reply':
        final completer = _replies.remove(message[1] as int);
        if (completer == null) return;
        if (message[2] == true) {
          completer.complete(message[3]);
        } else {
          completer.completeError(CoreError(message[3] as String, (message[4] as String?) ?? ''));
        }
      case 'event':
        _events.add(CoreEvent(Map<String, dynamic>.from(message[1] as Map)));
      case 'snapshot':
        _snapshots.add(message[1] as Snapshot);
    }
  }

  /// Команда ядра. Ошибка — [CoreError] с кодом.
  Future<dynamic> call(String cmd, [Map<String, Object?> args = const {}]) {
    final port = _commands;
    if (port == null) return Future<dynamic>.error(CoreError('not_started'));
    final id = _nextId++;
    final completer = Completer<dynamic>();
    _replies[id] = completer;
    port.send(<Object?>[id, cmd, args]);
    return completer.future;
  }

  /// На экране — частый опрос, в фоне — редкий.
  void setActive(bool active) => _commands?.send(<Object?>['active', active]);
}

/// Тело изолята ядра.
void _workerMain(List<Object?> args) {
  final ui = args[0] as SendPort;
  final init = (args[1] as Map).cast<String, Object?>();
  final Native native;
  try {
    native = Native.open();
    native.call('init', init);
  } catch (e, st) {
    ui.send(<Object?>['fatal', '$e\n\n$st']);
    return;
  }

  final inbox = ReceivePort();
  var version = -1;
  Timer? timer;

  void tick() {
    final Map<String, dynamic> polled;
    try {
      polled = native.poll();
    } catch (_) {
      return;
    }
    final events = polled['events'];
    if (events is List) {
      for (final e in events) {
        if (e is Map) ui.send(<Object?>['event', Map<String, dynamic>.from(e)]);
      }
    }
    final v = polled['v'];
    final current = v is num ? v.toInt() : 0;
    if (current == version) return;
    version = current;
    try {
      final r = native.call('snapshot');
      if (r is Map) ui.send(<Object?>['snapshot', Snapshot.fromJson(Map<String, dynamic>.from(r))]);
    } catch (_) {
      // Повторим на следующем такте.
      version = -1;
    }
  }

  void schedule(Duration every) {
    timer?.cancel();
    timer = Timer.periodic(every, (_) => tick());
  }

  inbox.listen((message) {
    if (message is! List || message.isEmpty) return;
    if (message[0] == 'active') {
      schedule(message[1] == true ? _activePoll : _backgroundPoll);
      return;
    }
    final id = message[0] as int;
    final cmd = message[1] as String;
    final cmdArgs = (message[2] as Map).cast<String, Object?>();
    try {
      ui.send(<Object?>['reply', id, true, native.call(cmd, cmdArgs), null]);
    } on CoreError catch (e) {
      ui.send(<Object?>['reply', id, false, e.code, e.detail]);
    } catch (e) {
      ui.send(<Object?>['reply', id, false, 'failed', '$e']);
    }
    // Команда меняет состояние — показать сразу, не дожидаясь такта.
    tick();
  });

  ui.send(<Object?>['ready', inbox.sendPort]);
  schedule(_activePoll);
  tick();
}
