// Размеры, скорости, «сколько осталось» и «когда был в сети» — по правилам
// текущего языка (intl).

import 'package:intl/intl.dart';

import '../l10n/l10n.dart';

/// 12,4 МБ / 12.4 MB.
String formatBytes(int bytes) {
  final units = tr.byteUnits;
  var value = bytes.toDouble();
  var i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i++;
  }
  final pattern = (i == 0 || value >= 100) ? '#,##0' : '#,##0.#';
  final number = NumberFormat(pattern, tr.localeName).format(value);
  return '$number ${units[i]}';
}

/// 5,2 МБ/с.
String formatRate(int bytesPerSecond) => tr.perSecond(formatBytes(bytesPerSecond));

/// 8 с, 2 мин, 1 ч 5 мин.
String formatDuration(int seconds) {
  if (seconds < 60) return tr.secondsShort(seconds < 1 ? 1 : seconds);
  final minutes = (seconds / 60).round();
  if (minutes < 60) return tr.minutesShort(minutes);
  final h = minutes ~/ 60;
  final m = minutes % 60;
  return m == 0 ? tr.hoursShort(h) : tr.hoursMinutesShort(h, m);
}

/// Сколько осталось при текущей скорости; null — не оценить.
String? formatLeft(int done, int total, int rate) {
  if (rate <= 0 || total <= done) return null;
  final seconds = ((total - done) / rate).ceil();
  if (seconds > 7 * 24 * 3600) return null;
  return formatDuration(seconds);
}

/// Ход передачи: «12 из 50 МБ · 5 МБ/с · ещё 8 с».
String formatProgress(int done, int total, int rate) {
  final left = formatLeft(done, total, rate);
  final d = formatBytes(done);
  final t = formatBytes(total);
  final r = formatRate(rate);
  return left == null ? tr.progress(d, t, r) : tr.progressLeft(d, t, r, left);
}

/// «только что», «5 мин назад», «вчера», дата.
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
  return DateFormat.yMd(tr.localeName).format(at);
}

/// «отчёт.pdf», «a.jpg, b.jpg и ещё 1», «Фото · 120 файлов».
String formatNames(List<String> names, int count) {
  if (names.isEmpty) return tr.files(count);
  final shown = names.take(2).join(', ');
  if (count <= names.length) {
    final more = count - (names.length < 2 ? names.length : 2);
    return more > 0 ? tr.andMore(shown, more) : shown;
  }
  return '$shown · ${tr.files(count)}';
}

/// «27 октября 23:15» — до какого момента действует ссылка.
String formatUntil(int unixSeconds) {
  final at = DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000);
  return DateFormat.MMMMd(tr.localeName).add_Hm().format(at);
}

/// М:сс для обратного отсчёта.
String formatClock(int seconds) {
  final s = seconds < 0 ? 0 : seconds;
  return '${s ~/ 60}:${(s % 60).toString().padLeft(2, '0')}';
}
