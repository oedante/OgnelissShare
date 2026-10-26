// Тексты интерфейса: русский и английский. Язык — по языку системы
// (русский и для белорусского, украинского, казахского; иначе английский).

import 'dart:ui' show Locale;

/// Текущие тексты: `tr.thisDevice`.
S get tr => S.current;

/// Русские формы множественного числа: 1 файл, 2 файла, 5 файлов.
String ruPlural(int n, String one, String few, String many) {
  final m10 = n % 10;
  final m100 = n % 100;
  if (m10 == 1 && m100 != 11) return one;
  if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return few;
  return many;
}

abstract class S {
  const S();

  static S current = const SRu();
  static Locale locale = const Locale('ru');

  static void load(Locale system) {
    const russianSpeaking = {'ru', 'be', 'uk', 'kk', 'ky', 'tg', 'uz', 'hy', 'az'};
    if (russianSpeaking.contains(system.languageCode)) {
      current = const SRu();
      locale = const Locale('ru');
    } else {
      current = const SEn();
      locale = const Locale('en');
    }
  }

  bool get isRu;

  String get appName => 'Ogneliss Share';

  // Общие действия
  String get save;
  String get cancel;
  String get close;
  String get done;
  String get retry;
  String get open;
  String get openFolder;
  String get copy;
  String get copied;
  String get accept;
  String get decline;
  String get connect;
  String get clear;
  String get settings;
  String get send;
  String get rename;

  // Это устройство
  String get thisDevice;
  String get deviceNameTitle;
  String get deviceNameHint;
  String get onlineInternet;
  String get onlineLocal;

  // Список устройств
  String get myDevices;
  String get oneTimePeers;
  String get online;
  String get offline;
  String get connecting;
  String lastSeen(String ago);
  String get sessionBadge;
  String get emptyTitle;
  String get emptyBody;
  String get showCode;
  String get scanCode;
  String get pasteLink;
  String get addDevice;

  // Передачи
  String get transfers;
  String get statusPreparing;
  String get statusConnecting;
  String get statusWaitingPeer;
  String get statusWaiting;
  String get statusAsking;
  String get statusRetrying;
  String get statusSent;
  String get statusReceived;
  String get statusDeclined;
  String get statusCancelled;
  String get statusCancelledByReceiver;
  String get statusCancelledBySender;
  String progress(String done, String total, String rate, String? left);
  String get textItem;
  String andMore(String list, int more);
  String files(int n);
  String failedFiles(int n);
  String get received;

  // Отправка
  String sendTo(String name);
  String get sendFiles;
  String get sendMedia;
  String get sendFolder;
  String get sendText;
  String get offlineNote;
  String get textHint;
  String get forget;
  String get endSession;
  String forgetConfirm(String name);
  String get paste;

  // Добавить устройство
  String get addTitle;
  String get modeMine;
  String get modeOnce;
  String get modeMineBody;
  String get modeOnceBody;
  String validFor(String time);
  String get copyLink;
  String get linkHint;
  String joinedPending(String name);
  String joinedDone(String name);
  String joinedOnce(String name);
  String joinedDeclined(String name);
  String get newCode;
  String get codeExpired;

  // Вопросы
  String pairRequestTitle(String name);
  String get pairRequestBody;
  String offerTitle(String name);

  // Сканирование
  String get scanTitle;
  String get scanHint;
  String get linkFieldHint;
  String get cameraUnavailable;
  String get desktopScanHint;

  // Подключение по коду
  String joining(String name);
  String get joinCheckCode;
  String joinDone(String name);
  String joinDoneOnce(String name);
  String get toDevices;

  // События
  String receivedFiles(String name, int n);
  String receivedText(String name);
  String peerRemoved(String name);
  String preparingShare(int n);
  String get shareChoose;
  String get shareNoDevices;

  // Настройки
  String get downloadsFolder;
  String get change;
  String get resetDefault;
  String get whereFiles;
  String get about;
  String get howItWorks;
  String get howItWorksBody;
  String get logs;
  String version(String v);
  String get license;
  String get sourceCode;
  String get relay;
  String get relayPublic;
  String get startupError;

  // Ошибки
  String error(String? code);

  // Единицы
  List<String> get byteUnits;
  String get perSecond;
  String seconds(int n);
  String minutes(int n);
  String hours(int h, int m);
  String get justNow;
  String minutesAgo(int n);
  String hoursAgo(int n);
  String get yesterday;
}

class SRu extends S {
  const SRu();

  @override
  bool get isRu => true;

  @override
  String get save => 'Сохранить';
  @override
  String get cancel => 'Отмена';
  @override
  String get close => 'Закрыть';
  @override
  String get done => 'Готово';
  @override
  String get retry => 'Повторить';
  @override
  String get open => 'Открыть';
  @override
  String get openFolder => 'Открыть папку';
  @override
  String get copy => 'Копировать';
  @override
  String get copied => 'Скопировано';
  @override
  String get accept => 'Принять';
  @override
  String get decline => 'Отклонить';
  @override
  String get connect => 'Подключить';
  @override
  String get clear => 'Очистить';
  @override
  String get settings => 'Настройки';
  @override
  String get send => 'Отправить';
  @override
  String get rename => 'Переименовать';

  @override
  String get thisDevice => 'Это устройство';
  @override
  String get deviceNameTitle => 'Имя устройства';
  @override
  String get deviceNameHint => 'Так это устройство увидят остальные';
  @override
  String get onlineInternet => 'в сети';
  @override
  String get onlineLocal => 'только локальная сеть';

  @override
  String get myDevices => 'Мои устройства';
  @override
  String get oneTimePeers => 'Разовый обмен';
  @override
  String get online => 'в сети';
  @override
  String get offline => 'не в сети';
  @override
  String get connecting => 'подключение…';
  @override
  String lastSeen(String ago) => 'был в сети $ago';
  @override
  String get sessionBadge => 'разовый';
  @override
  String get emptyTitle => 'Свяжите свои устройства';
  @override
  String get emptyBody => 'Один раз покажите код на одном устройстве и отсканируйте его другим. '
      'Дальше файлы идут напрямую и в зашифрованном виде — без аккаунтов и облака.';
  @override
  String get showCode => 'Показать код';
  @override
  String get scanCode => 'Сканировать код';
  @override
  String get pasteLink => 'Вставить ссылку';
  @override
  String get addDevice => 'Добавить устройство';

  @override
  String get transfers => 'Передачи';
  @override
  String get statusPreparing => 'Подготовка…';
  @override
  String get statusConnecting => 'Подключение…';
  @override
  String get statusWaitingPeer => 'Ждём, когда устройство появится в сети…';
  @override
  String get statusWaiting => 'Ждём ответа получателя…';
  @override
  String get statusAsking => 'Ждёт вашего решения';
  @override
  String get statusRetrying => 'Связь прервалась, восстанавливаем…';
  @override
  String get statusSent => 'Отправлено';
  @override
  String get statusReceived => 'Получено';
  @override
  String get statusDeclined => 'Получатель отказался';
  @override
  String get statusCancelled => 'Отменено';
  @override
  String get statusCancelledByReceiver => 'Отменено получателем';
  @override
  String get statusCancelledBySender => 'Отменено отправителем';
  @override
  String progress(String done, String total, String rate, String? left) =>
      '$done из $total · $rate${left == null ? '' : ' · ещё $left'}';
  @override
  String get textItem => 'Текст';
  @override
  String andMore(String list, int more) => more <= 0 ? list : '$list и ещё $more';
  @override
  String files(int n) => '$n ${ruPlural(n, 'файл', 'файла', 'файлов')}';
  @override
  String failedFiles(int n) => 'не прошли проверку: ${files(n)}';
  @override
  String get received => 'Получено';

  @override
  String sendTo(String name) => 'Отправить на «$name»';
  @override
  String get sendFiles => 'Файлы';
  @override
  String get sendMedia => 'Фото и видео';
  @override
  String get sendFolder => 'Папку';
  @override
  String get sendText => 'Текст или ссылку';
  @override
  String get offlineNote =>
      'Устройство сейчас не в сети. Передача начнётся, как только оно появится (ждём до 10 минут).';
  @override
  String get textHint => 'Текст или ссылка';
  @override
  String get forget => 'Забыть устройство';
  @override
  String get endSession => 'Завершить обмен';
  @override
  String forgetConfirm(String name) =>
      '«$name» и это устройство больше не смогут обмениваться файлами. Чтобы снова связаться, понадобится QR-код.';
  @override
  String get paste => 'Вставить';

  @override
  String get addTitle => 'Добавить устройство';
  @override
  String get modeMine => 'Моё устройство';
  @override
  String get modeOnce => 'Разовый обмен';
  @override
  String get modeMineBody => 'Для ваших устройств. Связь сохранится, файлы будут приниматься без вопросов. '
      'На втором устройстве откройте Ogneliss Share и нажмите «Сканировать код».';
  @override
  String get modeOnceBody => 'Для обмена с другим человеком. Связь — только на этот раз, '
      'каждую передачу получатель подтверждает.';
  @override
  String validFor(String time) => 'Код действует ещё $time';
  @override
  String get copyLink => 'Скопировать ссылку';
  @override
  String get linkHint => 'Нет камеры? Скопируйте ссылку и вставьте её на другом устройстве: '
      '«Сканировать код» → «Вставить ссылку».';
  @override
  String joinedPending(String name) => '«$name» подключается — подтвердите на этом устройстве.';
  @override
  String joinedDone(String name) => '«$name» добавлено в ваши устройства.';
  @override
  String joinedOnce(String name) => 'Подключено: «$name». Можно обмениваться файлами.';
  @override
  String joinedDeclined(String name) => 'Подключение «$name» отклонено.';
  @override
  String get newCode => 'Новый код';
  @override
  String get codeExpired => 'Код устарел';

  @override
  String pairRequestTitle(String name) => 'Подключить «$name»?';
  @override
  String get pairRequestBody =>
      'Проверьте, что на экране того устройства тот же код. Не совпадает — нажмите «Отклонить».';
  @override
  String offerTitle(String name) => '«$name» отправляет файлы';

  @override
  String get scanTitle => 'Сканировать код';
  @override
  String get scanHint => 'Наведите камеру на QR-код на экране другого устройства';
  @override
  String get linkFieldHint => 'Ссылка вида ogshare://…';
  @override
  String get cameraUnavailable => 'Камера недоступна. Вставьте ссылку, скопированную на другом устройстве.';
  @override
  String get desktopScanHint => 'На компьютере код удобнее показать, а отсканировать — телефоном. '
      'Или вставьте сюда ссылку, скопированную на другом устройстве.';

  @override
  String joining(String name) => 'Подключение к «$name»…';
  @override
  String get joinCheckCode => 'Сверьте код с экраном другого устройства и подтвердите там:';
  @override
  String joinDone(String name) => 'Готово! «$name» теперь среди ваших устройств.';
  @override
  String joinDoneOnce(String name) => 'Подключено к «$name». Можно обмениваться файлами.';
  @override
  String get toDevices => 'К устройствам';

  @override
  String receivedFiles(String name, int n) => 'От «$name» получено: ${files(n)}';
  @override
  String receivedText(String name) => 'Текст от «$name»';
  @override
  String peerRemoved(String name) => '«$name» больше не связано с этим устройством';
  @override
  String preparingShare(int n) => 'Подготовка: ${files(n)}…';
  @override
  String get shareChoose => 'Кому отправить?';
  @override
  String get shareNoDevices => 'Сначала добавьте устройство — покажите код или отсканируйте его.';

  @override
  String get downloadsFolder => 'Папка для полученных файлов';
  @override
  String get change => 'Изменить';
  @override
  String get resetDefault => 'По умолчанию';
  @override
  String get whereFiles => 'Где найти файлы';
  @override
  String get about => 'О приложении';
  @override
  String get howItWorks => 'Как это работает';
  @override
  String get howItWorksBody => 'Учётных записей нет. У каждого устройства свой ключ, созданный на нём самом; '
      'по ключу устройства и узнают друг друга.\n\n'
      'Связать устройства можно только QR-кодом, который показан на экране: посторонний '
      'не может ни подключиться, ни прислать файл — такие соединения отклоняются сразу.\n\n'
      'Файлы идут напрямую между устройствами и зашифрованы (QUIC, TLS 1.3). Если прямой путь '
      'невозможен, данные проходят через ретранслятор — он видит только зашифрованный поток.\n\n'
      'После обрыва связи передача продолжается с места остановки, каждый файл проверяется '
      'по хешу BLAKE3.';
  @override
  String get logs => 'Журнал';
  @override
  String version(String v) => 'Версия $v';
  @override
  String get license => 'Открытый код, лицензия Apache 2.0';
  @override
  String get sourceCode => 'Исходный код';
  @override
  String get relay => 'Ретранслятор';
  @override
  String get relayPublic => 'общедоступный (n0)';
  @override
  String get startupError => 'Не удалось запустить ядро приложения';

  @override
  String error(String? code) => switch (code) {
        'unreachable' => 'Устройство недоступно. Проверьте, что на нём открыт Ogneliss Share '
            'и есть интернет или общая сеть Wi-Fi.',
        'expired' => 'Код устарел или уже использован. Попросите показать новый.',
        'declined' => 'Отклонено на другом устройстве.',
        'busy' => 'Слишком много попыток подключения. Попробуйте через минуту.',
        'timeout' => 'Нет ответа от другого устройства.',
        'removed' => 'Связь с устройством разорвана.',
        'bad_link' => 'Это не код Ogneliss Share.',
        'own_link' => 'Это код этого же устройства.',
        'storage' => 'Не удалось сохранить файлы. Проверьте свободное место.',
        'interrupted' => 'Связь прервалась. Нажмите «Повторить» — передача продолжится с места обрыва.',
        'verify' => 'Часть файлов повредилась в пути и не сохранена. Нажмите «Повторить».',
        'read' => 'Не удалось прочитать файл.',
        'empty' => 'Нечего отправлять.',
        'too_big' => 'Слишком много файлов или слишком длинный текст.',
        'bad_path' => 'Недопустимые имена файлов.',
        'protocol' => 'На другом устройстве несовместимая версия приложения.',
        'not_found' => 'Устройство не найдено.',
        _ => 'Не получилось.',
      };

  @override
  List<String> get byteUnits => const ['Б', 'КБ', 'МБ', 'ГБ', 'ТБ'];
  @override
  String get perSecond => '/с';
  @override
  String seconds(int n) => '$n с';
  @override
  String minutes(int n) => '$n мин';
  @override
  String hours(int h, int m) => m == 0 ? '$h ч' : '$h ч $m мин';
  @override
  String get justNow => 'только что';
  @override
  String minutesAgo(int n) => '$n мин назад';
  @override
  String hoursAgo(int n) => '$n ч назад';
  @override
  String get yesterday => 'вчера';
}

class SEn extends S {
  const SEn();

  @override
  bool get isRu => false;

  @override
  String get save => 'Save';
  @override
  String get cancel => 'Cancel';
  @override
  String get close => 'Close';
  @override
  String get done => 'Done';
  @override
  String get retry => 'Retry';
  @override
  String get open => 'Open';
  @override
  String get openFolder => 'Open folder';
  @override
  String get copy => 'Copy';
  @override
  String get copied => 'Copied';
  @override
  String get accept => 'Accept';
  @override
  String get decline => 'Decline';
  @override
  String get connect => 'Connect';
  @override
  String get clear => 'Clear';
  @override
  String get settings => 'Settings';
  @override
  String get send => 'Send';
  @override
  String get rename => 'Rename';

  @override
  String get thisDevice => 'This device';
  @override
  String get deviceNameTitle => 'Device name';
  @override
  String get deviceNameHint => 'Your other devices will see this name';
  @override
  String get onlineInternet => 'online';
  @override
  String get onlineLocal => 'local network only';

  @override
  String get myDevices => 'My devices';
  @override
  String get oneTimePeers => 'One-time exchange';
  @override
  String get online => 'online';
  @override
  String get offline => 'offline';
  @override
  String get connecting => 'connecting…';
  @override
  String lastSeen(String ago) => 'last seen $ago';
  @override
  String get sessionBadge => 'one-time';
  @override
  String get emptyTitle => 'Link your devices';
  @override
  String get emptyBody => 'Show a code on one device and scan it with another — just once. '
      'After that, files go directly and encrypted — no accounts, no cloud.';
  @override
  String get showCode => 'Show code';
  @override
  String get scanCode => 'Scan code';
  @override
  String get pasteLink => 'Paste link';
  @override
  String get addDevice => 'Add device';

  @override
  String get transfers => 'Transfers';
  @override
  String get statusPreparing => 'Preparing…';
  @override
  String get statusConnecting => 'Connecting…';
  @override
  String get statusWaitingPeer => 'Waiting for the device to come online…';
  @override
  String get statusWaiting => 'Waiting for the receiver…';
  @override
  String get statusAsking => 'Waiting for your decision';
  @override
  String get statusRetrying => 'Connection lost, reconnecting…';
  @override
  String get statusSent => 'Sent';
  @override
  String get statusReceived => 'Received';
  @override
  String get statusDeclined => 'The receiver declined';
  @override
  String get statusCancelled => 'Cancelled';
  @override
  String get statusCancelledByReceiver => 'Cancelled by the receiver';
  @override
  String get statusCancelledBySender => 'Cancelled by the sender';
  @override
  String progress(String done, String total, String rate, String? left) =>
      '$done of $total · $rate${left == null ? '' : ' · $left left'}';
  @override
  String get textItem => 'Text';
  @override
  String andMore(String list, int more) => more <= 0 ? list : '$list and $more more';
  @override
  String files(int n) => '$n file${n == 1 ? '' : 's'}';
  @override
  String failedFiles(int n) => 'failed verification: ${files(n)}';
  @override
  String get received => 'Received';

  @override
  String sendTo(String name) => 'Send to “$name”';
  @override
  String get sendFiles => 'Files';
  @override
  String get sendMedia => 'Photos & videos';
  @override
  String get sendFolder => 'Folder';
  @override
  String get sendText => 'Text or link';
  @override
  String get offlineNote =>
      'The device is offline. The transfer will start as soon as it shows up (we wait up to 10 minutes).';
  @override
  String get textHint => 'Text or link';
  @override
  String get forget => 'Forget device';
  @override
  String get endSession => 'End exchange';
  @override
  String forgetConfirm(String name) =>
      '“$name” and this device will no longer be able to exchange files. You will need a QR code to link them again.';
  @override
  String get paste => 'Paste';

  @override
  String get addTitle => 'Add a device';
  @override
  String get modeMine => 'My device';
  @override
  String get modeOnce => 'One-time';
  @override
  String get modeMineBody => 'For your own devices. The link is kept and files are accepted without asking. '
      'On the other device open Ogneliss Share and tap “Scan code”.';
  @override
  String get modeOnceBody => 'For exchanging with another person. The link is temporary, '
      'and the receiver confirms every transfer.';
  @override
  String validFor(String time) => 'Code valid for $time';
  @override
  String get copyLink => 'Copy link';
  @override
  String get linkHint => 'No camera? Copy the link and paste it on the other device: '
      '“Scan code” → “Paste link”.';
  @override
  String joinedPending(String name) => '“$name” is connecting — confirm on this device.';
  @override
  String joinedDone(String name) => '“$name” added to your devices.';
  @override
  String joinedOnce(String name) => 'Connected: “$name”. You can exchange files now.';
  @override
  String joinedDeclined(String name) => 'Connection of “$name” declined.';
  @override
  String get newCode => 'New code';
  @override
  String get codeExpired => 'Code expired';

  @override
  String pairRequestTitle(String name) => 'Connect “$name”?';
  @override
  String get pairRequestBody =>
      'Make sure the other device shows the same code. If it does not match, tap “Decline”.';
  @override
  String offerTitle(String name) => '“$name” wants to send files';

  @override
  String get scanTitle => 'Scan code';
  @override
  String get scanHint => 'Point the camera at the QR code on the other device';
  @override
  String get linkFieldHint => 'A link like ogshare://…';
  @override
  String get cameraUnavailable => 'The camera is unavailable. Paste the link copied on the other device.';
  @override
  String get desktopScanHint => 'On a computer it is easier to show the code and scan it with a phone. '
      'Or paste the link copied on the other device here.';

  @override
  String joining(String name) => 'Connecting to “$name”…';
  @override
  String get joinCheckCode => 'Check that the other device shows the same code and confirm there:';
  @override
  String joinDone(String name) => 'Done! “$name” is now one of your devices.';
  @override
  String joinDoneOnce(String name) => 'Connected to “$name”. You can exchange files now.';
  @override
  String get toDevices => 'Back to devices';

  @override
  String receivedFiles(String name, int n) => 'Received from “$name”: ${files(n)}';
  @override
  String receivedText(String name) => 'Text from “$name”';
  @override
  String peerRemoved(String name) => '“$name” is no longer linked to this device';
  @override
  String preparingShare(int n) => 'Preparing ${files(n)}…';
  @override
  String get shareChoose => 'Send to…';
  @override
  String get shareNoDevices => 'Add a device first — show a code or scan one.';

  @override
  String get downloadsFolder => 'Folder for received files';
  @override
  String get change => 'Change';
  @override
  String get resetDefault => 'Default';
  @override
  String get whereFiles => 'Where to find files';
  @override
  String get about => 'About';
  @override
  String get howItWorks => 'How it works';
  @override
  String get howItWorksBody => 'There are no accounts. Every device has its own key created on the device '
      'itself; devices recognize each other by these keys.\n\n'
      'Devices can only be linked with a QR code shown on screen: a stranger can neither connect '
      'nor send a file — such connections are rejected right away.\n\n'
      'Files go directly between devices, encrypted (QUIC, TLS 1.3). If a direct path is impossible, '
      'data goes through a relay, which only sees the encrypted stream.\n\n'
      'After a connection drop the transfer resumes where it stopped, and every file is verified '
      'with a BLAKE3 hash.';
  @override
  String get logs => 'Log';
  @override
  String version(String v) => 'Version $v';
  @override
  String get license => 'Open source, Apache License 2.0';
  @override
  String get sourceCode => 'Source code';
  @override
  String get relay => 'Relay';
  @override
  String get relayPublic => 'public (n0)';
  @override
  String get startupError => 'Could not start the app core';

  @override
  String error(String? code) => switch (code) {
        'unreachable' => 'The device is unreachable. Make sure Ogneliss Share is open on it '
            'and it has internet or shares your Wi-Fi.',
        'expired' => 'The code has expired or was already used. Ask for a new one.',
        'declined' => 'Declined on the other device.',
        'busy' => 'Too many connection attempts. Try again in a minute.',
        'timeout' => 'No answer from the other device.',
        'removed' => 'The device is no longer linked.',
        'bad_link' => 'This is not an Ogneliss Share code.',
        'own_link' => 'This is the code of this very device.',
        'storage' => 'Could not save the files. Check free space.',
        'interrupted' => 'The connection was lost. Tap “Retry” — the transfer resumes where it stopped.',
        'verify' => 'Some files were damaged on the way and were not saved. Tap “Retry”.',
        'read' => 'Could not read the file.',
        'empty' => 'Nothing to send.',
        'too_big' => 'Too many files or the text is too long.',
        'bad_path' => 'Invalid file names.',
        'protocol' => 'The other device runs an incompatible version of the app.',
        'not_found' => 'Device not found.',
        _ => 'Something went wrong.',
      };

  @override
  List<String> get byteUnits => const ['B', 'KB', 'MB', 'GB', 'TB'];
  @override
  String get perSecond => '/s';
  @override
  String seconds(int n) => '$n s';
  @override
  String minutes(int n) => '$n min';
  @override
  String hours(int h, int m) => m == 0 ? '$h h' : '$h h $m min';
  @override
  String get justNow => 'just now';
  @override
  String minutesAgo(int n) => '$n min ago';
  @override
  String hoursAgo(int n) => '$n h ago';
  @override
  String get yesterday => 'yesterday';
}
