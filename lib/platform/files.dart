// Выбор файлов и папок для отправки и открытие полученного.

import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:open_filex/open_filex.dart';
import 'package:url_launcher/url_launcher.dart';

class Files {
  Files._();

  /// Несколько файлов. Пустой список — пользователь передумал.
  /// На телефонах плагин отдаёт копии во временной папке приложения.
  static Future<List<String>> pickFiles({bool media = false}) async {
    final picked = await FilePicker.pickFiles(type: media ? FileType.media : FileType.any);
    return [
      for (final f in picked)
        if (f.path != null) f.path!,
    ];
  }

  /// Папка целиком (на компьютерах).
  static Future<String?> pickFolder() => FilePicker.getDirectoryPath();

  /// Открыть файл или папку в системе.
  static Future<bool> open(String path) async {
    try {
      if (Platform.isAndroid || Platform.isIOS) {
        final r = await OpenFilex.open(path);
        return r.type == ResultType.done;
      }
      return await launchUrl(Uri.file(path, windows: Platform.isWindows));
    } catch (_) {
      return false;
    }
  }

  static bool exists(String path) {
    try {
      return FileSystemEntity.typeSync(path) != FileSystemEntityType.notFound;
    } catch (_) {
      return false;
    }
  }

  static bool isDirectory(String path) {
    try {
      return FileSystemEntity.isDirectorySync(path);
    } catch (_) {
      return false;
    }
  }
}
