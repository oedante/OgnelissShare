#!/usr/bin/env python3
"""Страницы Ogneliss Share на ogneliss.ru: описание, политика, приглашение.

    python3 site/build_share_site.py      # нужен Pillow (значки)

Пишет в site/out/ готовое содержимое для контейнера статики сайта (в том же
устройстве, что у остальных страниц ogneliss.ru):

  share/index.html     /share/           описание и скачивание (английский)
  share/privacy.html   /share/privacy    политика конфиденциальности
  share/join.html      /share/join/      страница ссылки-приглашения
  ru/share/…           /share/ru/…       то же по-русски
  share/app-icon.png, favicon.svg, favicon-32.png, apple-touch-icon.png

Правила nginx для этих адресов — site/nginx-share.conf, выкладка — site/README.md.
Оформление — как у остальных страниц сайта (стили site/style.css.txt), без
скриптов. Ссылка-приглашение передаётся после «#»: браузер её на сервер не
отправляет, поэтому страница /share/join/ её не видит и только объясняет,
что делать.
"""
import html
import re
import shutil
import sys
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
OUT = HERE / 'out'
BASE = 'https://ogneliss.ru'
EMAIL = 'oedante@gmail.com'
GITHUB = 'https://github.com/oedante/OgnelissShare'
RELEASES = GITHUB + '/releases/latest'
DL = GITHUB + '/releases/latest/download/'
LICENSE = GITHUB + '/blob/main/LICENSE'
ISSUES = GITHUB + '/issues'
OWNER = {'en': 'Ermishin Oleg Andreevich', 'ru': 'Ермишин Олег Андреевич'}
UPDATED = {'en': '31 October 2026', 'ru': '31 октября 2026 г.'}
LANGS = ['en', 'ru']

T = {
'en': dict(
    title='Ogneliss Share — send files between your devices directly',
    desc='Ogneliss Share sends files, folders, photos and text between your phone and computer directly and end-to-end encrypted. No cloud, no accounts. Android, iPhone, Windows, macOS, Linux.',
    tagline='Files between your devices — directly and encrypted',
    platforms='Android · iPhone · iPad · Windows · macOS · Linux',
    lead='Link your phone, tablet and computer once — with a QR code or a link sent by e-mail or in a messenger. After that, files, folders, photos and text go straight from one device to another, end-to-end encrypted. No cloud, no accounts, no size limits.',
    download='Download', source='Source code',
    free='Free and open source (Apache 2.0). No ads, no analytics.',
    get_h='Download',
    get=[('Android', 'Android 7.0 or later. <a href="{dl}ogneliss-share-android-arm64.apk">APK for most phones</a> · <a href="{dl}ogneliss-share-android-armv7.apk">older 32-bit phones</a>. Google Play — testing is starting.'),
         ('Windows', 'Windows 10 and 11, 64-bit. <a href="{dl}ogneliss-share-windows-x64.zip">Zip archive</a>: unpack and run ogneliss_share.exe. Allow access on private networks when Windows asks. Microsoft Store — later.'),
         ('macOS', '<a href="{dl}ogneliss-share-macos.zip">Test build</a> (not signed yet: right-click → Open). The Mac App Store version is coming through TestFlight.'),
         ('iPhone and iPad', 'Testing through TestFlight — write to <a href="mailto:{mail}">{mail}</a> to join.'),
         ('Linux', '<a href="{dl}ogneliss-share-linux-x64.tar.gz">tar.gz archive</a> for x86-64.')],
    all_releases='All versions and checksums — <a href="{rel}">GitHub Releases</a>.',
    how_h='How it works',
    steps=['Install Ogneliss Share on two devices.',
           'On one tap “Add a device” — a QR code appears. On the other tap “Scan code”. The device is far away or has no camera? Tap “Send a link” and send it by e-mail or in a messenger.',
           'Check that both screens show the same four digits and confirm.',
           'Done: tap the device in the list and choose what to send. Your own devices accept files on their own.'],
    features_h='Features',
    features=['Files, folders, photos and videos, text and links — in both directions, between any of the supported systems.',
              'On the same Wi-Fi network files go directly, at the speed of the network; over the internet your devices find each other on their own.',
              'A dropped connection does not lose the transfer: it resumes where it stopped. Every file is verified with a BLAKE3 hash.',
              '“My devices” accept files on their own; “Other people’s devices” stay linked, but you confirm every transfer; a “One-time exchange” lasts for a single session.',
              '“Send to all” — the same files, text or clipboard to every device of a group at once.',
              '“Files by link”: send a link, and the recipient downloads the files directly from your device while it is online (the link is valid for 24 hours and can be revoked).',
              'Online status of every device and your own names for them. On Android — “Share” from any app.'],
    security_h='Security',
    security='There are no accounts: every device has its own key created on the device itself. Devices can only be linked with a QR code shown on screen or an invitation link you sent yourself, and the inviting device confirms the connection. A stranger can neither connect nor send you a file — such connections are rejected during the handshake. Data is end-to-end encrypted (QUIC, TLS 1.3). When a direct path is impossible, it goes through a relay server that only sees the encrypted stream.',
    privacy_h='Privacy',
    privacy_short='The developer receives no data at all: no analytics, no crash reports, no advertising. Files go between devices and are not stored on servers. Details are in the <a href="{privacy}">Privacy Policy</a>.',
    oss_h='Open source',
    oss='Ogneliss Share is free software under the Apache License 2.0: everything said about security can be checked in the code. Source code, releases and issue tracker are on GitHub — bug reports and suggestions are welcome.',
    releases='Releases', issues='Report a problem', license='License (Apache 2.0)',
    dev_h='Developer', dev='{owner}, individual developer. Questions and support: <a href="mailto:{mail}">{mail}</a>.',
    privacy_title='Privacy Policy — Ogneliss Share',
    privacy_desc='How Ogneliss Share handles data: no accounts, no analytics, files go directly between devices end-to-end encrypted.',
    updated='Last updated: {date}',
    back='Ogneliss Share',
    join_title='Invitation to Ogneliss Share',
    join_desc='Someone invited you to exchange files in Ogneliss Share.',
    join_h='You have an Ogneliss Share invitation',
    join_lead='Someone sent you a link to link your devices or to receive files in Ogneliss Share — an app that sends files directly between devices, end-to-end encrypted.',
    join_steps_h='What to do',
    join_steps=['Install Ogneliss Share — links are below.',
                'Copy the link from the message whole (with everything after the “#” sign).',
                'In the app: “Scan code” → “Paste link”. On Android with the app installed, the link opens in the app by itself.',
                'If asked, compare the four digits with the person who sent the link.'],
    join_note='The part of the link after “#” never reaches this website: the browser keeps it to itself. The link works once (files by link — for 24 hours) and is useless to anyone else once used.',
    join_get_h='Install Ogneliss Share',
    langs_label='Language',
),
'ru': dict(
    title='Ogneliss Share — файлы между своими устройствами напрямую',
    desc='Ogneliss Share передаёт файлы, папки, фото и текст между телефоном и компьютером напрямую, со сквозным шифрованием. Без облака и учётных записей. Android, iPhone, Windows, macOS, Linux.',
    tagline='Файлы между своими устройствами — напрямую и с шифрованием',
    platforms='Android · iPhone · iPad · Windows · macOS · Linux',
    lead='Свяжите телефон, планшет и компьютер один раз — QR-кодом или ссылкой по почте или в мессенджере. Дальше файлы, папки, фото и текст идут с устройства на устройство напрямую, со сквозным шифрованием. Без облака, без учётных записей и без ограничений на размер.',
    download='Скачать', source='Исходный код',
    free='Бесплатно и с открытым кодом (Apache 2.0). Без рекламы и аналитики.',
    get_h='Скачать',
    get=[('Android', 'Android 7.0 и новее. <a href="{dl}ogneliss-share-android-arm64.apk">APK для большинства телефонов</a> · <a href="{dl}ogneliss-share-android-armv7.apk">для старых 32-битных</a>. Google Play — начинается тестирование.'),
         ('Windows', 'Windows 10 и 11, 64 бита. <a href="{dl}ogneliss-share-windows-x64.zip">Архив zip</a>: распакуйте и запустите ogneliss_share.exe. Когда Windows спросит, разрешите доступ в частных сетях. Microsoft Store — позже.'),
         ('macOS', '<a href="{dl}ogneliss-share-macos.zip">Тестовая сборка</a> (пока без подписи: правый щелчок → «Открыть»). Версия для Mac App Store готовится через TestFlight.'),
         ('iPhone и iPad', 'Тестирование через TestFlight — напишите на <a href="mailto:{mail}">{mail}</a>, чтобы присоединиться.'),
         ('Linux', '<a href="{dl}ogneliss-share-linux-x64.tar.gz">Архив tar.gz</a> для x86-64.')],
    all_releases='Все версии и контрольные суммы — <a href="{rel}">GitHub Releases</a>.',
    how_h='Как это работает',
    steps=['Установите Ogneliss Share на два устройства.',
           'На одном нажмите «Добавить устройство» — появится QR-код. На другом — «Сканировать код». Устройство далеко или без камеры? Нажмите «Отправить ссылкой» и пошлите её по почте или в мессенджере.',
           'Проверьте, что на обоих экранах одинаковые четыре цифры, и подтвердите.',
           'Готово: нажмите на устройство в списке и выберите, что отправить. Свои устройства принимают файлы сами.'],
    features_h='Возможности',
    features=['Файлы, папки, фото и видео, текст и ссылки — в обе стороны, между любыми из поддерживаемых систем.',
              'В одной сети Wi-Fi файлы идут напрямую, со скоростью сети; через интернет устройства находят друг друга сами.',
              'Обрыв связи не губит передачу: она продолжится с места остановки. Каждый файл проверяется по хешу BLAKE3.',
              '«Мои устройства» принимают файлы сами; «Чужие устройства» связаны постоянно, но каждую передачу вы подтверждаете; «Разовый обмен» — на один сеанс.',
              '«Отправить всем» — одни и те же файлы, текст или буфер обмена сразу на все устройства группы.',
              '«Файлы по ссылке»: отправьте ссылку — получатель скачает файлы прямо с вашего устройства, пока оно в сети (ссылка действует сутки, её можно отозвать).',
              'Видно, какие устройства в сети; им можно дать свои имена. На Android — «Поделиться» из любого приложения.'],
    security_h='Безопасность',
    security='Учётных записей нет: у каждого устройства свой ключ, созданный на нём самом. Связать устройства можно только QR-кодом с экрана или ссылкой-приглашением, которую вы отправили сами, а подключение подтверждается на пригласившем устройстве. Посторонний не может ни подключиться, ни прислать файл — такие соединения отклоняются ещё при рукопожатии. Данные зашифрованы сквозным шифрованием (QUIC, TLS 1.3). Если прямой путь невозможен, они идут через ретранслятор, который видит только зашифрованный поток.',
    privacy_h='Конфиденциальность',
    privacy_short='Разработчик не получает никаких данных: ни аналитики, ни отчётов о сбоях, ни рекламы. Файлы идут между устройствами и не хранятся на серверах. Подробно — в <a href="{privacy}">Политике конфиденциальности</a>.',
    oss_h='Открытый код',
    oss='Ogneliss Share — свободная программа под лицензией Apache 2.0: всё сказанное о безопасности можно проверить по коду. Исходный код, выпуски и список проблем — на GitHub; сообщения об ошибках и предложения приветствуются.',
    releases='Выпуски', issues='Сообщить о проблеме', license='Лицензия (Apache 2.0)',
    dev_h='Разработчик', dev='{owner}, частный разработчик. Вопросы и поддержка: <a href="mailto:{mail}">{mail}</a>.',
    privacy_title='Политика конфиденциальности — Ogneliss Share',
    privacy_desc='Как Ogneliss Share обращается с данными: без учётных записей и аналитики, файлы идут напрямую между устройствами со сквозным шифрованием.',
    updated='Обновлено: {date}',
    back='Ogneliss Share',
    join_title='Приглашение в Ogneliss Share',
    join_desc='Вас пригласили обменяться файлами в Ogneliss Share.',
    join_h='Вам прислали приглашение Ogneliss Share',
    join_lead='Вам прислали ссылку, чтобы связать устройства или получить файлы в Ogneliss Share — приложении, которое передаёт файлы напрямую между устройствами со сквозным шифрованием.',
    join_steps_h='Что сделать',
    join_steps=['Установите Ogneliss Share — ссылки ниже.',
                'Скопируйте ссылку из сообщения целиком (со всем, что после знака «#»).',
                'В приложении: «Сканировать код» → «Вставить ссылку». На Android с установленным приложением ссылка откроется в нём сама.',
                'Если попросят, сверьте четыре цифры с тем, кто прислал ссылку.'],
    join_note='Часть ссылки после «#» на этот сайт не попадает: браузер оставляет её у себя. Ссылка срабатывает один раз (файлы по ссылке — в течение суток), и после этого никому другому не пригодится.',
    join_get_h='Установить Ogneliss Share',
    langs_label='Язык',
),
}

# ── Политика конфиденциальности: (заголовок, [абзацы]) ──
PRIVACY = {
'en': [
    ('In short', ['Ogneliss Share has no accounts, no analytics, no advertising and no crash reporting. The developer does not receive, store or process any of your data. Files, texts and device names travel only between the devices taking part in an exchange, end-to-end encrypted.']),
    ('Who is responsible', ['{owner}, individual developer (“the developer”). Contact: <a href="mailto:{mail}">{mail}</a>.']),
    ('What is stored on your device', [
        'The device key: a secret key created on the device at first start. It never leaves the device and is excluded from backups.',
        'The list of linked devices (public key, name, platform, the name you gave it, when it was last online and its last known network addresses), active invitation links and settings.',
        'Received files — in the folder you see in the app (Downloads/Ogneliss Share by default; on iPhone — Files → On My iPhone → Ogneliss Share).',
        'The technical log is kept in memory only while the app runs and is shown to you on request; it is not saved or sent anywhere.']),
    ('What is sent over the network and to whom', [
        '<strong>To other devices.</strong> Files, texts, device names and the list of offered files go only to the device you send them to, over an end-to-end encrypted connection (QUIC, TLS 1.3). Nobody else, including the developer, can read them.',
        '<strong>Relay servers.</strong> When devices cannot connect directly, the encrypted stream goes through public relay servers of number 0 (n0), the maker of the iroh networking library. While the app runs, it also keeps a connection to its “home” relay so that your devices can reach it. A relay sees the IP addresses and public keys of the devices, but not the content.',
        '<strong>Address lookup.</strong> So that linked devices find each other after moving between networks, the app publishes in the public n0 address service (dns.iroh.link) a record signed by the device key: “public key → home relay address”. The record contains no IP addresses of the device and no names. Looking up another device’s record also goes to this service.',
        '<strong>Local network.</strong> On a local network the app announces its public key and addresses so that your devices find each other without the internet (not on iPhone and iPad).',
        '<strong>Invitation links.</strong> A link contains the public key, the device name, a one-time secret and the relay address. You send it yourself through the e-mail or messenger you choose; their privacy policies apply to the message. When such a link is opened in a browser, the part after “#” is not sent to ogneliss.ru.']),
    ('What the app does not do', ['No analytics, tracking, advertising or third-party data-collection SDKs; no access to contacts, location or the microphone. The camera is used only to scan pairing QR codes; files are read only when you choose them.']),
    ('Permissions', ['Camera — to scan QR codes. Network and local network — to connect your devices. Files and photos — only the ones you pick in the system dialog.']),
    ('Storage and deletion', ['All app data is on your device. “Forget device” removes a device from the list on both sides; uninstalling the app deletes the key and the list. Received files stay in their folder until you delete them. Records in the address service stop being updated when the app is removed and expire on their own.']),
    ('Children', ['The app is not directed at children and does not collect data from anyone.']),
    ('Changes', ['Changes to this policy are published on this page with a new date. Questions — <a href="mailto:{mail}">{mail}</a>.']),
],
'ru': [
    ('Коротко', ['В Ogneliss Share нет учётных записей, аналитики, рекламы и отчётов о сбоях. Разработчик не получает, не хранит и не обрабатывает ваших данных. Файлы, тексты и имена устройств идут только между устройствами, которые участвуют в обмене, со сквозным шифрованием.']),
    ('Кто отвечает', ['{owner}, частный разработчик («разработчик»). Связь: <a href="mailto:{mail}">{mail}</a>.']),
    ('Что хранится на устройстве', [
        'Ключ устройства: секретный ключ, созданный на устройстве при первом запуске. Он не покидает устройство и не попадает в резервные копии.',
        'Список связанных устройств (открытый ключ, имя, платформа, имя, которое вы дали, когда было в сети и последние известные сетевые адреса), действующие ссылки-приглашения и настройки.',
        'Полученные файлы — в папке, которую показывает приложение (по умолчанию «Загрузки/Ogneliss Share»; на iPhone — «Файлы» → «На iPhone» → Ogneliss Share).',
        'Технический журнал хранится только в памяти, пока приложение запущено, и показывается вам по запросу; он никуда не записывается и не отправляется.']),
    ('Что передаётся по сети и кому', [
        '<strong>Другим устройствам.</strong> Файлы, тексты, имена устройств и список предлагаемых файлов уходят только тому устройству, которому вы их отправляете, по соединению со сквозным шифрованием (QUIC, TLS 1.3). Никто другой, включая разработчика, прочитать их не может.',
        '<strong>Ретрансляторы.</strong> Когда устройства не могут связаться напрямую, зашифрованный поток идёт через общедоступные ретрансляторы компании number 0 (n0), разработчика сетевой библиотеки iroh. Пока приложение работает, оно держит связь со своим «домашним» ретранслятором, чтобы ваши устройства могли до него достучаться. Ретранслятор видит IP-адреса и открытые ключи устройств, но не содержимое.',
        '<strong>Поиск адресов.</strong> Чтобы связанные устройства находили друг друга после смены сети, приложение публикует в общедоступной службе адресов n0 (dns.iroh.link) запись, подписанную ключом устройства: «открытый ключ → адрес домашнего ретранслятора». IP-адресов устройства и имён в ней нет. Поиск записи другого устройства тоже идёт в эту службу.',
        '<strong>Локальная сеть.</strong> В локальной сети приложение сообщает свой открытый ключ и адреса, чтобы ваши устройства находили друг друга без интернета (кроме iPhone и iPad).',
        '<strong>Ссылки-приглашения.</strong> В ссылке — открытый ключ, имя устройства, одноразовый секрет и адрес ретранслятора. Её отправляете вы сами через выбранные почту или мессенджер; к сообщению применяются их правила. Когда такая ссылка открывается в браузере, часть после «#» на ogneliss.ru не передаётся.']),
    ('Чего приложение не делает', ['Нет аналитики, слежения, рекламы и сторонних SDK для сбора данных; нет доступа к контактам, местоположению и микрофону. Камера нужна только для сканирования QR-кодов; файлы читаются, только когда вы их выбираете.']),
    ('Разрешения', ['Камера — сканировать QR-коды. Сеть и локальная сеть — связывать ваши устройства. Файлы и фото — только те, что вы выбрали в системном окне.']),
    ('Хранение и удаление', ['Все данные приложения — на вашем устройстве. «Забыть устройство» убирает его из списка на обеих сторонах; удаление приложения удаляет ключ и список. Полученные файлы остаются в папке, пока вы их не удалите. Записи в службе адресов перестают обновляться после удаления приложения и устаревают сами.']),
    ('Дети', ['Приложение не предназначено специально для детей и ни у кого не собирает данные.']),
    ('Изменения', ['Изменения политики публикуются на этой странице с новой датой. Вопросы — <a href="mailto:{mail}">{mail}</a>.']),
],
}


def url(lang, page):
    """page: index | privacy | join."""
    l = '' if lang == 'en' else f'{lang}/'
    return {'index': f'/share/{l}', 'privacy': f'/share/{l}privacy', 'join': f'/share/{l}join/'}[page]


def out_file(lang, page):
    l = '' if lang == 'en' else f'{lang}/'
    return f'{l}share/{page}.html'


CSS = re.sub(r'\s*\n\s*', '', re.sub(r'/\*.*?\*/', '', (HERE / 'style.css.txt').read_text(encoding='utf-8'), flags=re.S))


def page(lang, pg, title, desc, body, noindex=False):
    t = T[lang]
    alts = '\n'.join(f'<link rel="alternate" hreflang="{l}" href="{BASE}{url(l, pg)}">' for l in LANGS)
    langs = ''.join(f'<a href="{url(l, pg)}" hreflang="{l}" lang="{l}"' + (' aria-current="true"' if l == lang else '')
                    + f'>{l.upper()}</a>' for l in LANGS)
    robots = '<meta name="robots" content="noindex">\n' if noindex else ''
    return f'''<!DOCTYPE html>
<html lang="{lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<!-- Generated by site/build_share_site.py (OgnelissShare repository). -->
<title>{html.escape(title)}</title>
<meta name="description" content="{html.escape(desc)}">
{robots}<link rel="canonical" href="{BASE}{url(lang, pg)}">
{alts}
<link rel="alternate" hreflang="x-default" href="{BASE}{url('en', pg)}">
<link rel="icon" href="/share/favicon.svg" type="image/svg+xml">
<link rel="alternate icon" href="/share/favicon-32.png" sizes="32x32">
<link rel="apple-touch-icon" href="/share/apple-touch-icon.png">
<meta property="og:title" content="{html.escape(title)}">
<meta property="og:description" content="{html.escape(desc)}">
<meta property="og:url" content="{BASE}{url(lang, pg)}">
<meta property="og:type" content="website">
<style>{CSS}</style>
</head>
<body>
<header class="bar">
<div class="wrap">
<a class="brand" href="{'/' if lang == 'en' else f'/{lang}/'}"><img src="/dialer/logo.svg" width="30" height="30" alt="">Ogneliss</a>
<nav class="nav" aria-label="Ogneliss Share"><a href="{url(lang, 'index')}" aria-current="page">Ogneliss Share</a></nav>
<nav class="langs" aria-label="{t['langs_label']}">{langs}</nav>
</div>
</header>
<main>
{body}
</main>
<footer class="foot">
<div class="wrap">
<div class="cols">
<div><h2>Ogneliss Share</h2><ul><li><a href="{url(lang, 'index')}">{t['back']}</a></li><li><a href="{url(lang, 'privacy')}">{t['privacy_h']}</a></li><li><a href="{GITHUB}">GitHub</a></li><li><a href="{RELEASES}">{t['releases']}</a></li></ul></div>
<div><h2>{t['dev_h']}</h2><ul><li>{OWNER[lang]}</li><li><a href="mailto:{EMAIL}">{EMAIL}</a></li></ul></div>
</div>
<div class="bottom"><span>© 2026 {OWNER[lang]}</span><span><a href="{'/' if lang == 'en' else f'/{lang}/'}">ogneliss.ru</a></span></div>
</div>
</footer>
</body>
</html>
'''


def fill(s, lang):
    return s.format(dl=DL, mail=EMAIL, rel=RELEASES, owner=OWNER[lang], privacy=url(lang, 'privacy'),
                    date=UPDATED[lang])


def downloads(lang):
    t = T[lang]
    cards = '\n'.join(f'<div class="card"><h3>{h}</h3><p>{fill(d, lang)}</p></div>' for h, d in t['get'])
    return f'<div class="grid">\n{cards}\n</div>\n<p class="hint">{fill(t["all_releases"], lang)}</p>'


def index(lang):
    t = T[lang]
    li = lambda xs: '\n'.join(f'<li>{x}</li>' for x in xs)  # noqa: E731
    body = f'''<section class="hero">
<div class="wrap">
<div class="app-head">
<img src="/share/app-icon.png" width="88" height="88" alt="Ogneliss Share">
<div>
<h1>Ogneliss Share</h1>
<p class="tag">{t['tagline']}</p>
<span class="platforms">{t['platforms']}</span>
</div>
</div>
<p class="lead">{t['lead']}</p>
<div class="actions">
<a class="btn primary" href="#download">{t['download']}</a>
<a class="btn ghost" href="{GITHUB}">{t['source']}</a>
</div>
<p class="links">{t['free']}</p>
</div>
</section>
<div class="wrap">
<section class="section" id="download">
<h2>{t['get_h']}</h2>
{downloads(lang)}
</section>
<section class="section">
<h2>{t['how_h']}</h2>
<div class="card"><ol>
{li(t['steps'])}
</ol></div>
</section>
<section class="section">
<h2>{t['features_h']}</h2>
<ul class="features two">
{li(t['features'])}
</ul>
</section>
<section class="section" id="security">
<h2>{t['security_h']}</h2>
<div class="card"><p>{t['security']}</p></div>
</section>
<section class="section" id="privacy">
<h2>{t['privacy_h']}</h2>
<div class="card"><p>{fill(t['privacy_short'], lang)}</p></div>
</section>
<section class="section" id="source">
<h2>{t['oss_h']}</h2>
<div class="card"><p>{t['oss']}</p>
<p><a href="{GITHUB}">GitHub</a> · <a href="{RELEASES}">{t['releases']}</a> · <a href="{ISSUES}">{t['issues']}</a> · <a href="{LICENSE}">{t['license']}</a></p></div>
</section>
<section class="section">
<h2>{t['dev_h']}</h2>
<div class="card"><p>{fill(t['dev'], lang)}</p></div>
</section>
</div>'''
    return page(lang, 'index', t['title'], t['desc'], body)


def privacy(lang):
    t = T[lang]
    parts = []
    for h, paras in PRIVACY[lang]:
        parts.append(f'<h2>{h}</h2>\n' + '\n'.join(f'<p>{fill(p, lang)}</p>' for p in paras))
    body = f'''<section class="hero doc-head">
<div class="wrap narrow">
<div class="crumbs"><a href="{'/' if lang == 'en' else f'/{lang}/'}">Ogneliss</a> › <a href="{url(lang, 'index')}">Ogneliss Share</a></div>
<h1>{t['privacy_h']}</h1>
<p class="meta">Ogneliss Share · {fill(t['updated'], lang)}</p>
</div>
</section>
<div class="wrap narrow">
<article class="doc">
{chr(10).join(parts)}
</article>
</div>'''
    return page(lang, 'privacy', t['privacy_title'], t['privacy_desc'], body)


def join(lang):
    t = T[lang]
    li = lambda xs: '\n'.join(f'<li>{x}</li>' for x in xs)  # noqa: E731
    body = f'''<section class="hero">
<div class="wrap">
<div class="app-head">
<img src="/share/app-icon.png" width="88" height="88" alt="Ogneliss Share">
<div>
<h1>{t['join_h']}</h1>
<span class="platforms">{t['platforms']}</span>
</div>
</div>
<p class="lead">{t['join_lead']}</p>
</div>
</section>
<div class="wrap">
<section class="section">
<h2>{t['join_steps_h']}</h2>
<div class="card"><ol>
{li(t['join_steps'])}
</ol></div>
<div class="note"><p>{t['join_note']}</p></div>
</section>
<section class="section">
<h2>{t['join_get_h']}</h2>
{downloads(lang)}
</section>
<section class="section">
<div class="card"><p><a href="{url(lang, 'index')}">Ogneliss Share</a> · <a href="{url(lang, 'privacy')}">{t['privacy_h']}</a></p></div>
</section>
</div>'''
    return page(lang, 'join', t['join_title'], t['join_desc'], body, noindex=True)


def icons():
    d = OUT / 'share'
    d.mkdir(parents=True, exist_ok=True)
    rounded = Image.open(ROOT / 'assets' / 'icons' / 'app_icon_rounded.png').convert('RGBA')
    rounded.resize((256, 256), Image.LANCZOS).save(d / 'app-icon.png', optimize=True)
    rounded.resize((32, 32), Image.LANCZOS).save(d / 'favicon-32.png', optimize=True)
    full = Image.open(ROOT / 'assets' / 'icons' / 'app_icon.png').convert('RGB')
    full.resize((180, 180), Image.LANCZOS).save(d / 'apple-touch-icon.png', optimize=True)
    shutil.copyfile(ROOT / 'assets' / 'brand' / 'logo.svg', d / 'favicon.svg')


def check(out):
    bad = []
    known = {url(l, p) for l in LANGS for p in ('index', 'privacy', 'join')}
    assets = {'/share/app-icon.png', '/share/favicon.svg', '/share/favicon-32.png', '/share/apple-touch-icon.png',
              '/dialer/logo.svg', '/', '/ru/'}
    for path, text in out.items():
        if re.search(r'<script|display:\s*none', re.sub(r'<!--.*?-->', '', text, flags=re.S)):
            bad.append(f'{path}: скрипт или скрытый блок')
        if text.count('<h1') != 1:
            bad.append(f'{path}: h1 не один')
        if not path.startswith('ru/'):
            v = re.sub(r'<a [^>]*lang="ru"[^>]*>[^<]*</a>', '', text).replace(OWNER['ru'], '')
            if re.search(r'[А-Яа-яЁё]', re.sub(r'<!--.*?-->', '', v, flags=re.S)):
                bad.append(f'{path}: кириллица на английской странице')
        for href in re.findall(r'(?:href|src)="(/[^"#]*)', text):
            if href not in known and href not in assets:
                bad.append(f'{path}: неизвестная ссылка {href}')
        if '{' in re.sub(r'<style>.*?</style>', '', text, flags=re.S):
            bad.append(f'{path}: незаполненное {{…}}')
    return bad


def main():
    if OUT.exists():
        shutil.rmtree(OUT)
    out = {}
    for l in LANGS:
        out[out_file(l, 'index')] = index(l)
        out[out_file(l, 'privacy')] = privacy(l)
        out[out_file(l, 'join')] = join(l)
    for path, text in out.items():
        p = OUT / path
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding='utf-8')
    icons()
    bad = check(out)
    for b in bad:
        print('  ⚠', b, file=sys.stderr)
    print(f'Записано страниц: {len(out)} в {OUT.relative_to(ROOT)}' if not bad else f'Замечаний: {len(bad)}')
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
