// Запуск ядра: где хранить данные, куда класть полученные файлы и как
// назвать устройство по умолчанию.

import 'dart:async';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/widgets.dart';
import 'package:path/path.dart' as p;
import 'package:path_provider/path_provider.dart';

import '../app/app_model.dart';
import '../core/core_client.dart';
import 'platform_bridge.dart';

bool get isMobile => Platform.isAndroid || Platform.isIOS;

bool get isDesktop => Platform.isWindows || Platform.isMacOS || Platform.isLinux;

/// Сканер QR-кодов есть на телефонах и Mac (камера через системные средства).
bool get canScan => Platform.isAndroid || Platform.isIOS || Platform.isMacOS;

String get platformCode {
  if (Platform.isAndroid) return 'android';
  if (Platform.isIOS) return 'ios';
  if (Platform.isMacOS) return 'macos';
  if (Platform.isWindows) return 'windows';
  if (Platform.isLinux) return 'linux';
  return 'other';
}

/// Запустить ядро. Пути и имя устройства узнаются здесь (каналы платформы
/// работают только в основном изоляте), а само ядро — в своём изоляте.
Future<AppModel> startApp() async {
  final data = await getApplicationSupportDirectory();
  final downloads = await defaultDownloadDir();
  final name = await defaultDeviceName();
  final core = await CoreClient.start({
    'data_dir': data.path,
    'download_dir': downloads,
    'name': name,
    'platform': platformCode,
  });
  if (Platform.isAndroid || Platform.isIOS) {
    // Копии файлов, выбранных в прошлый раз, больше не нужны.
    unawaited(FilePicker.clearTemporaryFiles().catchError((_) {}));
  }
  return AppModel(core);
}

/// Куда сохранять полученное.
///   Android — «Загрузки/Ogneliss Share» (без разрешений с Android 11),
///             иначе папка приложения на общей памяти;
///   iOS — «Документы» приложения (видны в «Файлах» → «На iPhone»);
///   компьютер — «Загрузки/Ogneliss Share».
Future<String> defaultDownloadDir() async {
  if (Platform.isAndroid) {
    final public = await PlatformBridge.publicDownloads();
    if (public != null) {
      final dir = Directory(p.join(public, 'Ogneliss Share'));
      if (await _writable(dir)) return dir.path;
    }
    try {
      final ext = await getExternalStorageDirectory();
      if (ext != null) return p.join(ext.path, 'Received');
    } catch (_) {}
    return p.join((await getApplicationDocumentsDirectory()).path, 'Received');
  }
  if (Platform.isIOS) return (await getApplicationDocumentsDirectory()).path;
  Directory? downloads;
  try {
    downloads = await getDownloadsDirectory();
  } catch (_) {}
  final home = Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'] ?? '.';
  return p.join(downloads?.path ?? p.join(home, 'Downloads'), 'Ogneliss Share');
}

Future<bool> _writable(Directory dir) async {
  try {
    await dir.create(recursive: true);
    final probe = File(p.join(dir.path, '.probe-${DateTime.now().microsecondsSinceEpoch}'));
    await probe.writeAsString('ok', flush: true);
    await probe.delete();
    return true;
  } catch (_) {
    return false;
  }
}

/// Имя по умолчанию; пользователь может его поменять.
Future<String> defaultDeviceName() async {
  if (Platform.isAndroid) {
    final name = await PlatformBridge.deviceName();
    return (name == null || name.trim().isEmpty) ? 'Android' : name.trim();
  }
  if (Platform.isIOS) {
    final views = WidgetsBinding.instance.platformDispatcher.views;
    if (views.isNotEmpty) {
      final view = views.first;
      final shortest = view.physicalSize.shortestSide / view.devicePixelRatio;
      if (shortest >= 600) return 'iPad';
    }
    return 'iPhone';
  }
  var host = '';
  try {
    host = Platform.localHostname.trim();
  } catch (_) {}
  if (Platform.isMacOS) {
    host = host.replaceAll(RegExp(r'\.local$'), '').replaceAll('-', ' ').trim();
  }
  if (host.isEmpty || host == 'localhost') {
    if (Platform.isMacOS) return 'Mac';
    if (Platform.isWindows) return 'Windows';
    return 'Linux';
  }
  return host;
}
