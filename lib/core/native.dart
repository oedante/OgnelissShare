// Привязка к ядру на Rust (библиотека ogneliss_core, каталог rust/).
//
// У ядра три функции C:
//   osh_call(json) -> json  — команда и её результат;
//   osh_poll() -> json      — номер версии состояния и накопившиеся события;
//   osh_free(ptr)           — освободить строку, которую вернуло ядро.
// Команды быстрые (долгое ядро делает в своих потоках), поэтому вызываются
// прямо из потока интерфейса.

import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

import 'package:ffi/ffi.dart';

typedef _CallFn = Pointer<Utf8> Function(Pointer<Utf8>);
typedef _PollFn = Pointer<Utf8> Function();
typedef _FreeNative = Void Function(Pointer<Utf8>);
typedef _FreeDart = void Function(Pointer<Utf8>);

/// Ошибка ядра: короткий код («unreachable», «expired»…) и подробности для журнала.
class CoreError implements Exception {
  CoreError(this.code, [this.detail = '']);

  final String code;
  final String detail;

  @override
  String toString() => detail.isEmpty ? 'CoreError($code)' : 'CoreError($code: $detail)';
}

class Native {
  Native._(DynamicLibrary lib)
      : _call = lib.lookupFunction<_CallFn, _CallFn>('osh_call'),
        _poll = lib.lookupFunction<_PollFn, _PollFn>('osh_poll'),
        _free = lib.lookupFunction<_FreeNative, _FreeDart>('osh_free');

  /// Загрузить библиотеку ядра. Бросает исключение, если её нет в сборке.
  factory Native.open() => Native._(_openLibrary());

  final _CallFn _call;
  final _PollFn _poll;
  final _FreeDart _free;

  static DynamicLibrary _openLibrary() {
    const name = 'ogneliss_core';
    if (Platform.isIOS || Platform.isMacOS) {
      // Плагин собирается фреймворком (CocoaPods, use_frameworks!); если
      // библиотека вшита прямо в приложение — ищем функции в процессе.
      try {
        return DynamicLibrary.open('$name.framework/$name');
      } catch (_) {
        return DynamicLibrary.process();
      }
    }
    if (Platform.isAndroid || Platform.isLinux) return DynamicLibrary.open('lib$name.so');
    if (Platform.isWindows) return DynamicLibrary.open('$name.dll');
    throw UnsupportedError('Платформа не поддерживается: ${Platform.operatingSystem}');
  }

  String _take(Pointer<Utf8> ptr) {
    if (ptr == nullptr) throw CoreError('failed', 'пустой ответ ядра');
    try {
      return ptr.toDartString();
    } finally {
      _free(ptr);
    }
  }

  /// Выполнить команду ядра. Возвращает поле result ответа или бросает [CoreError].
  dynamic call(String cmd, [Map<String, Object?> args = const {}]) {
    final request = jsonEncode(<String, Object?>{'cmd': cmd, ...args});
    final req = request.toNativeUtf8();
    final String text;
    try {
      text = _take(_call(req));
    } finally {
      malloc.free(req);
    }
    final decoded = jsonDecode(text);
    if (decoded is! Map) throw CoreError('failed', text);
    if (decoded['ok'] == true) return decoded['result'];
    throw CoreError(
      (decoded['error'] as String?) ?? 'failed',
      (decoded['detail'] as String?) ?? '',
    );
  }

  /// Номер версии состояния и разовые события.
  Map<String, dynamic> poll() {
    final decoded = jsonDecode(_take(_poll()));
    if (decoded is Map<String, dynamic>) return decoded;
    return const <String, dynamic>{'v': 0, 'events': <dynamic>[]};
  }
}
