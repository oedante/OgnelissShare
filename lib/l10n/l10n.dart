// Локализация интерфейса.
//
// Все тексты — в ARB-файлах этой папки: app_en.arb (шаблон) и app_ru.arb.
// Классы AppLocalizations создаёт `flutter gen-l10n` (app_localizations*.dart,
// в git не хранятся). Таблица текстов для правки — tool/l10n/make_arb.py.
//
// В виджетах можно брать AppLocalizations.of(context); там, где контекста
// нет (уведомления, форматирование размеров и дат), — глобальный [tr], его
// обновляет MaterialApp.builder при смене языка.

import 'package:flutter/widgets.dart';

import 'app_localizations.dart';

export 'app_localizations.dart';

AppLocalizations? _current;

/// Тексты на текущем языке приложения.
AppLocalizations get tr => _current ??= lookupAppLocalizations(const Locale('en'));

void setTr(AppLocalizations value) => _current = value;

/// Языки, для которых интерфейс показывается по-русски.
const _russianSpeaking = {'ru', 'be', 'uk', 'kk', 'ky', 'tg', 'uz', 'hy', 'az'};

/// Язык приложения по списку языков системы: первый подходящий —
/// английский или русскоязычный; иначе английский.
Locale appLocale(List<Locale>? system) {
  for (final locale in system ?? const <Locale>[]) {
    if (locale.languageCode == 'en') return const Locale('en');
    if (_russianSpeaking.contains(locale.languageCode)) return const Locale('ru');
  }
  return const Locale('en');
}

extension AppTexts on AppLocalizations {
  /// Сообщение по коду ошибки ядра (см. node::error_code в rust/src/node.rs).
  String error(String? code) => switch (code) {
        'unreachable' => errorUnreachable,
        'expired' => errorExpired,
        'declined' => errorDeclined,
        'busy' => errorBusy,
        'timeout' => errorTimeout,
        'removed' => errorRemoved,
        'bad_link' => errorBadLink,
        'own_link' => errorOwnLink,
        'storage' => errorStorage,
        'no_space' => errorNoSpace,
        'interrupted' => errorInterrupted,
        'verify' => errorVerify,
        'read' => errorRead,
        'empty' => errorEmpty,
        'too_big' => errorTooBig,
        'bad_path' => errorBadPath,
        'protocol' => errorProtocol,
        'not_found' => errorNotFound,
        'too_many' => errorTooMany,
        _ => errorGeneric,
      };

  /// Б, КБ, МБ, ГБ, ТБ.
  List<String> get byteUnits => [unitB, unitKB, unitMB, unitGB, unitTB];

  /// Имя устройства или «Без имени».
  String deviceName(String name) => name.trim().isEmpty ? unnamedDevice : name;
}
