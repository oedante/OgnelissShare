// Размеры, скорости, «сколько осталось» и «когда был в сети».

import 'strings.dart';

/// 12,4 МБ / 12.4 MB.
String formatBytes(int bytes) {
  final units = tr.byteUnits;
  var value = bytes.toDouble();
  var i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i++;
  }
  final String number;
  if (i == 0) {
    number = '$bytes';
  } else if (value >= 100) {
    number = value.toStringAsFixed(0);
  } else {
    number = value.toStringAsFixed(1);
  }
  final localized = tr.isRu ? number.replaceAll('.', ',') : number;
  return '$localized ${units[i]}';
}

/// 5,2 МБ/с.
String formatRate(int bytesPerSecond) => '${formatBytes(bytesPerSecond)}${tr.perSecond}';

/// 8 с, 2 мин, 1 ч 5 мин.
String formatDuration(int seconds) {
  if (seconds < 60) return tr.seconds(seconds < 1 ? 1 : seconds);
  final minutes = (seconds / 60).round();
  if (minutes < 60) return tr.minutes(minutes);
  return tr.hours(minutes ~/ 60, minutes % 60);
}

/// Сколько осталось при текущей скорости; null — не оценить.
String? formatLeft(int done, int total, int rate) {
  if (rate <= 0 || total <= done) return null;
  final seconds = ((total - done) / rate).ceil();
  if (seconds > 7 * 24 * 3600) return null;
  return formatDuration(seconds);
}

/// «только что», «5 мин назад», «вчера», «12.10.2026».
String formatAgo(int unixSeconds, {DateTime? now}) {
  if (unixSeconds <= 0) return '';
  final at = DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000);
  final current = now ?? DateTime.now();
  final diff = current.difference(at);
  if (diff.inSeconds < 60) return tr.justNow;
  if (diff.inMinutes < 60) return tr.minutesAgo(diff.inMinutes);
  final today = DateTime(current.year, current.month, current.day);
  final day = DateTime(at.year, at.month, at.day);
  final days = today.difference(day).inDays;
  if (days == 0) return tr.hoursAgo(diff.inHours);
  if (days == 1) return tr.yesterday;
  String two(int v) => v.toString().padLeft(2, '0');
  return tr.isRu ? '${two(at.day)}.${two(at.month)}.${at.year}' : '${at.year}-${two(at.month)}-${two(at.day)}';
}

/// «отчёт.pdf», «a.jpg, b.jpg и ещё 1», «Фото · 120 файлов».
String formatNames(List<String> names, int count) {
  if (names.isEmpty) return tr.files(count);
  final shown = names.take(2).join(', ');
  if (count <= names.length) {
    final more = count - (names.length < 2 ? names.length : 2);
    return tr.andMore(shown, more);
  }
  return '$shown · ${tr.files(count)}';
}

/// Мм:сс для обратного отсчёта.
String formatClock(int seconds) {
  final s = seconds < 0 ? 0 : seconds;
  return '${s ~/ 60}:${(s % 60).toString().padLeft(2, '0')}';
}
