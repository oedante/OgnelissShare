#!/usr/bin/env python3
"""Единая таблица текстов → lib/l10n/app_en.arb и app_ru.arb.

Тексты правятся здесь (или прямо в ARB — тогда эту таблицу обновить тоже):
одна таблица не даёт ключам разойтись между языками.

    python3 tool/l10n/make_arb.py
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'lib' / 'l10n'

S = 'String'
I = 'int'

# ключ: (английский, русский, описание, {заполнитель: тип})
T = {
    'appName': ('Ogneliss Share', 'Ogneliss Share', 'Название приложения (не переводится)', {}),

    # Общие действия
    'save': ('Save', 'Сохранить', 'Кнопка', {}),
    'cancel': ('Cancel', 'Отмена', 'Кнопка', {}),
    'close': ('Close', 'Закрыть', 'Кнопка', {}),
    'done': ('Done', 'Готово', 'Кнопка', {}),
    'retry': ('Retry', 'Повторить', 'Кнопка', {}),
    'open': ('Open', 'Открыть', 'Кнопка: открыть файл', {}),
    'openFolder': ('Open folder', 'Открыть папку', 'Кнопка', {}),
    'copy': ('Copy', 'Копировать', 'Кнопка', {}),
    'copied': ('Copied', 'Скопировано', 'Уведомление', {}),
    'accept': ('Accept', 'Принять', 'Кнопка: принять файлы', {}),
    'decline': ('Decline', 'Отклонить', 'Кнопка', {}),
    'connect': ('Connect', 'Подключить', 'Кнопка', {}),
    'clear': ('Clear', 'Очистить', 'Кнопка: очистить список передач', {}),
    'settings': ('Settings', 'Настройки', 'Заголовок', {}),
    'send': ('Send', 'Отправить', 'Кнопка', {}),
    'rename': ('Rename', 'Переименовать', 'Кнопка', {}),
    'paste': ('Paste', 'Вставить', 'Кнопка: вставить из буфера', {}),
    'starting': ('Starting…', 'Запуск…', 'Пока запускается ядро', {}),

    # Это устройство
    'thisDevice': ('This device', 'Это устройство', 'Заголовок карточки своего устройства', {}),
    'deviceNameTitle': ('Device name', 'Имя устройства', 'Заголовок окна переименования', {}),
    'deviceNameHint': ('Your other devices will see this name', 'Так это устройство увидят остальные', 'Подсказка', {}),
    'unnamedDevice': ('Unnamed device', 'Без имени', 'Если устройство не сообщило имя', {}),
    'onlineInternet': ('online', 'в сети', 'Своё устройство подключено к интернету', {}),
    'onlineLocal': ('local network only', 'только локальная сеть', 'Нет связи с ретранслятором', {}),

    # Список устройств
    'myDevices': ('My devices', 'Мои устройства', 'Раздел', {}),
    'oneTimePeers': ('One-time exchange', 'Разовый обмен', 'Раздел', {}),
    'online': ('online', 'в сети', 'Состояние устройства', {}),
    'offline': ('offline', 'не в сети', 'Состояние устройства', {}),
    'connecting': ('connecting…', 'подключение…', 'Состояние устройства', {}),
    'lastSeen': ('last seen {ago}', 'был в сети {ago}', '«когда» — «5 мин назад», «вчера»', {'ago': S}),
    'sessionBadge': ('one-time', 'разовый', 'Плашка у разового собеседника', {}),
    'emptyTitle': ('Link your devices', 'Свяжите свои устройства', 'Пустой список', {}),
    'emptyBody': ('Show a code on one device and scan it with another — just once. After that, files go directly and encrypted — no accounts, no cloud.',
                  'Один раз покажите код на одном устройстве и отсканируйте его другим. Дальше файлы идут напрямую и в зашифрованном виде — без аккаунтов и облака.',
                  'Пустой список', {}),
    'showCode': ('Show code', 'Показать код', 'Кнопка', {}),
    'scanCode': ('Scan code', 'Сканировать код', 'Кнопка', {}),
    'pasteLink': ('Paste link', 'Вставить ссылку', 'Кнопка на компьютере без камеры', {}),
    'addDevice': ('Add device', 'Добавить устройство', 'Кнопка', {}),

    # Передачи
    'transfers': ('Transfers', 'Передачи', 'Раздел', {}),
    'statusPreparing': ('Preparing…', 'Подготовка…', 'Состояние передачи', {}),
    'statusConnecting': ('Connecting…', 'Подключение…', 'Состояние передачи', {}),
    'statusWaitingPeer': ('Waiting for the device to come online…', 'Ждём, когда устройство появится в сети…', 'Состояние передачи', {}),
    'statusWaiting': ('Waiting for the receiver…', 'Ждём ответа получателя…', 'Состояние передачи', {}),
    'statusAsking': ('Waiting for your decision', 'Ждёт вашего решения', 'Состояние передачи', {}),
    'statusRetrying': ('Connection lost, reconnecting…', 'Связь прервалась, восстанавливаем…', 'Состояние передачи', {}),
    'statusSent': ('Sent', 'Отправлено', 'Состояние передачи', {}),
    'statusReceived': ('Received', 'Получено', 'Состояние передачи', {}),
    'statusDeclined': ('The receiver declined', 'Получатель отказался', 'Состояние передачи', {}),
    'statusCancelled': ('Cancelled', 'Отменено', 'Состояние передачи', {}),
    'statusCancelledByReceiver': ('Cancelled by the receiver', 'Отменено получателем', 'Состояние передачи', {}),
    'statusCancelledBySender': ('Cancelled by the sender', 'Отменено отправителем', 'Состояние передачи', {}),
    'progress': ('{done} of {total} · {rate}', '{done} из {total} · {rate}', 'Ход передачи', {'done': S, 'total': S, 'rate': S}),
    'progressLeft': ('{done} of {total} · {rate} · {left} left', '{done} из {total} · {rate} · ещё {left}', 'Ход передачи с оценкой', {'done': S, 'total': S, 'rate': S, 'left': S}),
    'textItem': ('Text', 'Текст', 'Передача текста', {}),
    'textPreview': ('Text: “{text}”', 'Текст: «{text}»', 'Начало переданного текста', {'text': S}),
    'andMore': ('{list} and {more} more', '{list} и ещё {more}', '«a.jpg, b.jpg и ещё 3»', {'list': S, 'more': I}),
    'files': ('{count, plural, =1{1 file} other{{count} files}}', '{count, plural, one{{count} файл} few{{count} файла} many{{count} файлов} other{{count} файла}}', 'Число файлов', {'count': I}),
    'failedFiles': ('failed verification: {count, plural, =1{1 file} other{{count} files}}', 'не прошли проверку: {count, plural, one{{count} файл} few{{count} файла} many{{count} файлов} other{{count} файла}}', 'Файлы с несовпавшим хешем', {'count': I}),

    # Отправка
    'sendTo': ('Send to “{name}”', 'Отправить на «{name}»', 'Заголовок', {'name': S}),
    'sendFiles': ('Files', 'Файлы', 'Пункт меню', {}),
    'sendMedia': ('Photos & videos', 'Фото и видео', 'Пункт меню', {}),
    'sendFolder': ('Folder', 'Папку', 'Пункт меню', {}),
    'sendText': ('Text or link', 'Текст или ссылку', 'Пункт меню', {}),
    'offlineNote': ('The device is offline. The transfer will start as soon as it shows up (we wait up to 10 minutes).',
                    'Устройство сейчас не в сети. Передача начнётся, как только оно появится (ждём до 10 минут).', 'Подсказка', {}),
    'textHint': ('Text or link', 'Текст или ссылка', 'Подсказка поля', {}),
    'forget': ('Forget device', 'Забыть устройство', 'Пункт меню', {}),
    'endSession': ('End exchange', 'Завершить обмен', 'Пункт меню', {}),
    'forgetConfirm': ('“{name}” and this device will no longer be able to exchange files. You will need a QR code to link them again.',
                      '«{name}» и это устройство больше не смогут обмениваться файлами. Чтобы снова связаться, понадобится QR-код.',
                      'Подтверждение', {'name': S}),

    # Добавить устройство
    'addTitle': ('Add a device', 'Добавить устройство', 'Заголовок', {}),
    'modeMine': ('My device', 'Моё устройство', 'Режим', {}),
    'modeOnce': ('One-time', 'Разовый обмен', 'Режим', {}),
    'modeMineBody': ('For your own devices. The link is kept and files are accepted without asking. On the other device open Ogneliss Share and tap “Scan code”.',
                     'Для ваших устройств. Связь сохранится, файлы будут приниматься без вопросов. На втором устройстве откройте Ogneliss Share и нажмите «Сканировать код».',
                     'Пояснение режима', {}),
    'modeOnceBody': ('For exchanging with another person. The link is temporary, and the receiver confirms every transfer.',
                     'Для обмена с другим человеком. Связь — только на этот раз, каждую передачу получатель подтверждает.',
                     'Пояснение режима', {}),
    'validFor': ('Code valid for {time}', 'Код действует ещё {time}', 'Обратный отсчёт м:сс', {'time': S}),
    'copyLink': ('Copy link', 'Скопировать ссылку', 'Кнопка', {}),
    'linkHint': ('No camera? Copy the link and paste it on the other device: “Scan code” → “Paste link”.',
                 'Нет камеры? Скопируйте ссылку и вставьте её на другом устройстве: «Сканировать код» → «Вставить ссылку».',
                 'Подсказка', {}),
    'joinedPending': ('“{name}” is connecting — confirm on this device.', '«{name}» подключается — подтвердите на этом устройстве.', 'Состояние кода', {'name': S}),
    'joinedDone': ('“{name}” added to your devices.', '«{name}» добавлено в ваши устройства.', 'Состояние кода', {'name': S}),
    'joinedOnce': ('Connected: “{name}”. You can exchange files now.', 'Подключено: «{name}». Можно обмениваться файлами.', 'Состояние кода', {'name': S}),
    'joinedDeclined': ('Connection of “{name}” declined.', 'Подключение «{name}» отклонено.', 'Состояние кода', {'name': S}),
    'newCode': ('New code', 'Новый код', 'Кнопка', {}),
    'codeExpired': ('Code expired', 'Код устарел', 'Состояние кода', {}),

    # Вопросы
    'pairRequestTitle': ('Connect “{name}”?', 'Подключить «{name}»?', 'Вопрос', {'name': S}),
    'pairRequestBody': ('Make sure the other device shows the same code. If it does not match, tap “Decline”.',
                        'Проверьте, что на экране того устройства тот же код. Не совпадает — нажмите «Отклонить».', 'Вопрос', {}),
    'offerTitle': ('“{name}” wants to send files', '«{name}» отправляет файлы', 'Вопрос', {'name': S}),

    # Сканирование и подключение
    'scanTitle': ('Scan code', 'Сканировать код', 'Заголовок', {}),
    'scanHint': ('Point the camera at the QR code on the other device', 'Наведите камеру на QR-код на экране другого устройства', 'Подсказка', {}),
    'linkFieldHint': ('A link like ogshare://…', 'Ссылка вида ogshare://…', 'Подсказка поля', {}),
    'cameraUnavailable': ('The camera is unavailable. Paste the link copied on the other device.',
                          'Камера недоступна. Вставьте ссылку, скопированную на другом устройстве.', 'Ошибка камеры', {}),
    'desktopScanHint': ('On a computer it is easier to show the code and scan it with a phone. Or paste the link copied on the other device here.',
                        'На компьютере код удобнее показать, а отсканировать — телефоном. Или вставьте сюда ссылку, скопированную на другом устройстве.',
                        'Подсказка', {}),
    'joining': ('Connecting to “{name}”…', 'Подключение к «{name}»…', 'Состояние', {'name': S}),
    'joinCheckCode': ('Check that the other device shows the same code and confirm there:',
                      'Сверьте код с экраном другого устройства и подтвердите там:', 'Подсказка', {}),
    'joinDone': ('Done! “{name}” is now one of your devices.', 'Готово! «{name}» теперь среди ваших устройств.', 'Итог', {'name': S}),
    'joinDoneOnce': ('Connected to “{name}”. You can exchange files now.', 'Подключено к «{name}». Можно обмениваться файлами.', 'Итог', {'name': S}),
    'toDevices': ('Back to devices', 'К устройствам', 'Кнопка', {}),

    # События
    'receivedFiles': ('Received from “{name}”: {count, plural, =1{1 file} other{{count} files}}',
                      'От «{name}» получено: {count, plural, one{{count} файл} few{{count} файла} many{{count} файлов} other{{count} файла}}',
                      'Уведомление', {'name': S, 'count': I}),
    'receivedText': ('Text from “{name}”', 'Текст от «{name}»', 'Заголовок', {'name': S}),
    'peerRemoved': ('“{name}” is no longer linked to this device', '«{name}» больше не связано с этим устройством', 'Уведомление', {'name': S}),
    'preparingShare': ('Preparing {count, plural, =1{1 file} other{{count} files}}…',
                       'Подготовка: {count, plural, one{{count} файл} few{{count} файла} many{{count} файлов} other{{count} файла}}…',
                       'Уведомление', {'count': I}),
    'shareChoose': ('Send to…', 'Кому отправить?', 'Заголовок', {}),
    'shareNoDevices': ('Add a device first — show a code or scan one.', 'Сначала добавьте устройство — покажите код или отсканируйте его.', 'Подсказка', {}),

    # Настройки
    'downloadsFolder': ('Folder for received files', 'Папка для полученных файлов', 'Раздел', {}),
    'change': ('Change', 'Изменить', 'Кнопка', {}),
    'resetDefault': ('Default', 'По умолчанию', 'Кнопка', {}),
    'whereFiles': ('Where to find files', 'Где найти файлы', 'Раздел', {}),
    'whereFilesAndroid': ('Files → Downloads → Ogneliss Share', 'Файлы → Загрузки → Ogneliss Share', 'Путь к файлам на Android', {}),
    'whereFilesIos': ('Files → On My iPhone → Ogneliss Share', 'Файлы → На iPhone → Ogneliss Share', 'Путь к файлам на iOS', {}),
    'about': ('About', 'О приложении', 'Раздел', {}),
    'howItWorks': ('How it works', 'Как это работает', 'Раздел', {}),
    'howItWorksBody': (
        'There are no accounts. Every device has its own key created on the device itself; devices recognize each other by these keys.\n\n'
        'Devices can only be linked with a QR code shown on screen: a stranger can neither connect nor send a file — such connections are rejected right away.\n\n'
        'Files go directly between devices, encrypted (QUIC, TLS 1.3). If a direct path is impossible, data goes through a relay, which only sees the encrypted stream.\n\n'
        'After a connection drop the transfer resumes where it stopped, and every file is verified with a BLAKE3 hash.',
        'Учётных записей нет. У каждого устройства свой ключ, созданный на нём самом; по ключу устройства и узнают друг друга.\n\n'
        'Связать устройства можно только QR-кодом, который показан на экране: посторонний не может ни подключиться, ни прислать файл — такие соединения отклоняются сразу.\n\n'
        'Файлы идут напрямую между устройствами и зашифрованы (QUIC, TLS 1.3). Если прямой путь невозможен, данные проходят через ретранслятор — он видит только зашифрованный поток.\n\n'
        'После обрыва связи передача продолжается с места остановки, каждый файл проверяется по хешу BLAKE3.',
        'Пояснение', {}),
    'logs': ('Log', 'Журнал', 'Пункт', {}),
    'version': ('Version {version}', 'Версия {version}', 'Версия приложения', {'version': S}),
    'license': ('Open source, Apache License 2.0', 'Открытый код, лицензия Apache 2.0', 'Пункт', {}),
    'legalese': ('© 2026 Oleg Ermishin · Apache License 2.0', '© 2026 Ермишин Олег · лицензия Apache 2.0', 'Правообладатель', {}),
    'sourceCode': ('Source code', 'Исходный код', 'Пункт', {}),
    'website': ('Website', 'Сайт', 'Пункт', {}),
    'relay': ('Relay', 'Ретранслятор', 'Пункт', {}),
    'relayPublic': ('public (n0)', 'общедоступный (n0)', 'Ретранслятор по умолчанию', {}),
    'startupError': ('Could not start the app core', 'Не удалось запустить ядро приложения', 'Ошибка запуска', {}),

    # Ошибки ядра (коды — в node::error_code)
    'errorUnreachable': ('The device is unreachable. Make sure Ogneliss Share is open on it and it has internet or shares your Wi-Fi.',
                         'Устройство недоступно. Проверьте, что на нём открыт Ogneliss Share и есть интернет или общая сеть Wi-Fi.', 'Ошибка', {}),
    'errorExpired': ('The code has expired or was already used. Ask for a new one.', 'Код устарел или уже использован. Попросите показать новый.', 'Ошибка', {}),
    'errorDeclined': ('Declined on the other device.', 'Отклонено на другом устройстве.', 'Ошибка', {}),
    'errorBusy': ('Too many connection attempts. Try again in a minute.', 'Слишком много попыток подключения. Попробуйте через минуту.', 'Ошибка', {}),
    'errorTimeout': ('No answer from the other device.', 'Нет ответа от другого устройства.', 'Ошибка', {}),
    'errorRemoved': ('The device is no longer linked.', 'Связь с устройством разорвана.', 'Ошибка', {}),
    'errorBadLink': ('This is not an Ogneliss Share code.', 'Это не код Ogneliss Share.', 'Ошибка', {}),
    'errorOwnLink': ('This is the code of this very device.', 'Это код этого же устройства.', 'Ошибка', {}),
    'errorStorage': ('Could not save the files.', 'Не удалось сохранить файлы.', 'Ошибка', {}),
    'errorNoSpace': ('Not enough free space on the receiving device.', 'На устройстве получателя не хватает места.', 'Ошибка', {}),
    'errorInterrupted': ('The connection was lost. Tap “Retry” — the transfer resumes where it stopped.',
                         'Связь прервалась. Нажмите «Повторить» — передача продолжится с места обрыва.', 'Ошибка', {}),
    'errorVerify': ('Some files were damaged on the way and were not saved. Tap “Retry”.',
                    'Часть файлов повредилась в пути и не сохранена. Нажмите «Повторить».', 'Ошибка', {}),
    'errorRead': ('Could not read the file.', 'Не удалось прочитать файл.', 'Ошибка', {}),
    'errorEmpty': ('Nothing to send.', 'Нечего отправлять.', 'Ошибка', {}),
    'errorTooBig': ('Too many files or the text is too long.', 'Слишком много файлов или слишком длинный текст.', 'Ошибка', {}),
    'errorBadPath': ('Invalid file names.', 'Недопустимые имена файлов.', 'Ошибка', {}),
    'errorProtocol': ('The other device runs an incompatible version of the app.', 'На другом устройстве несовместимая версия приложения.', 'Ошибка', {}),
    'errorNotFound': ('Device not found.', 'Устройство не найдено.', 'Ошибка', {}),
    'errorGeneric': ('Something went wrong.', 'Не получилось.', 'Ошибка', {}),

    # Единицы
    'unitB': ('B', 'Б', 'байт', {}),
    'unitKB': ('KB', 'КБ', 'килобайт', {}),
    'unitMB': ('MB', 'МБ', 'мегабайт', {}),
    'unitGB': ('GB', 'ГБ', 'гигабайт', {}),
    'unitTB': ('TB', 'ТБ', 'терабайт', {}),
    'perSecond': ('{size}/s', '{size}/с', 'Скорость', {'size': S}),
    'secondsShort': ('{n} s', '{n} с', 'Длительность', {'n': I}),
    'minutesShort': ('{n} min', '{n} мин', 'Длительность', {'n': I}),
    'hoursShort': ('{h} h', '{h} ч', 'Длительность', {'h': I}),
    'hoursMinutesShort': ('{h} h {m} min', '{h} ч {m} мин', 'Длительность', {'h': I, 'm': I}),
    'justNow': ('just now', 'только что', 'Время', {}),
    'minutesAgo': ('{n} min ago', '{n} мин назад', 'Время', {'n': I}),
    'hoursAgo': ('{n} h ago', '{n} ч назад', 'Время', {'n': I}),
    'yesterday': ('yesterday', 'вчера', 'Время', {}),

    # Системные запросы разрешений iOS и macOS. В Dart не используются:
    # tool/bootstrap.py переносит их в Info.plist и InfoPlist.strings.
    'permissionCamera': ('The camera is used to scan the QR code of another device.',
                         'Камера нужна, чтобы отсканировать QR-код другого устройства.',
                         'Системный запрос доступа к камере (NSCameraUsageDescription)', {}),
    'permissionLocalNetwork': ('Ogneliss Share sends files directly between your devices on the local network.',
                               'Ogneliss Share передаёт файлы напрямую между вашими устройствами в локальной сети.',
                               'Системный запрос доступа к локальной сети (NSLocalNetworkUsageDescription)', {}),
}


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    en = {'@@locale': 'en'}
    ru = {'@@locale': 'ru'}
    for key, (e, r, desc, ph) in T.items():
        en[key] = e
        meta = {'description': desc}
        if ph:
            meta['placeholders'] = {name: {'type': typ} for name, typ in ph.items()}
        en['@' + key] = meta
        ru[key] = r
    for name, data in (('app_en.arb', en), ('app_ru.arb', ru)):
        (OUT / name).write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(f'{len(T)} ключей')


if __name__ == '__main__':
    main()
