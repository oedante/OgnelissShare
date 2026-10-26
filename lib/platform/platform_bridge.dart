// Мост к коду Android (MainActivity.kt): имя устройства, папка «Загрузки»,
// «Поделиться» из других приложений и в них, открытые ссылки-приглашения.
// На остальных платформах — пусто.

import 'dart:io';

import 'package:flutter/services.dart';

/// Пришло через «Поделиться»: файлы (копии в кэше приложения) и/или текст.
class SharedPayload {
  const SharedPayload({this.files = const [], this.text});

  final List<String> files;
  final String? text;

  bool get isEmpty => files.isEmpty && (text == null || text!.trim().isEmpty);
}

class PlatformBridge {
  PlatformBridge._();

  static const _channel = MethodChannel('ogneliss_share/platform');

  static Future<T?> _invoke<T>(String method) async {
    if (!Platform.isAndroid) return null;
    try {
      return await _channel.invokeMethod<T>(method);
    } catch (_) {
      return null;
    }
  }

  /// Имя телефона из настроек Android («Galaxy S24 Олега») или модель.
  static Future<String?> deviceName() => _invoke<String>('deviceName');

  /// Общая папка «Загрузки» (/storage/emulated/0/Download).
  static Future<String?> publicDownloads() => _invoke<String>('publicDownloads');

  /// Подписаться на «Поделиться» и ссылки: [onPreparing] — файлы копируются,
  /// [onShared] — можно забрать их через [takeShared], [onLink] — открыли
  /// ссылку-приглашение, забрать через [takeLink].
  static void listen({
    required void Function(int count) onPreparing,
    required void Function() onShared,
    required void Function() onLink,
  }) {
    if (!Platform.isAndroid) return;
    _channel.setMethodCallHandler((call) async {
      switch (call.method) {
        case 'sharing':
          final n = call.arguments;
          onPreparing(n is int ? n : 0);
        case 'shared':
          onShared();
        case 'link':
          onLink();
      }
      return null;
    });
  }

  /// Ссылка-приглашение, которой открыли приложение (и при запуске тоже).
  static Future<String?> takeLink() => _invoke<String>('takeLink');

  /// Системное «Поделиться» с текстом (почта, мессенджеры). false — не вышло
  /// или не Android: тогда интерфейс предлагает скопировать текст.
  static Future<bool> shareText(String subject, String text) async {
    if (!Platform.isAndroid) return false;
    try {
      final ok = await _channel.invokeMethod<bool>('shareText', {'subject': subject, 'text': text});
      return ok ?? false;
    } catch (_) {
      return false;
    }
  }

  /// Забрать то, что пришло через «Поделиться» (и при запуске приложения тоже).
  static Future<SharedPayload?> takeShared() async {
    if (!Platform.isAndroid) return null;
    try {
      final m = await _channel.invokeMapMethod<String, dynamic>('takeShared');
      if (m == null) return null;
      final raw = m['files'];
      final files = raw is List ? [for (final f in raw) if (f is String) f] : <String>[];
      final text = m['text'];
      final payload = SharedPayload(files: files, text: text is String ? text : null);
      return payload.isEmpty ? null : payload;
    } catch (_) {
      return null;
    }
  }
}
