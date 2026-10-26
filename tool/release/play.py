#!/usr/bin/env python3
"""Загрузка в Google Play через Google Play Developer API.

    pip install google-api-python-client google-auth

    # сборка → внутреннее тестирование, заметки из store/whats_new/<версия>.txt
    python tool/release/play.py dist/ogneliss-share-0.1.1-android.aab --key play-key.json

    # другая дорожка: alpha — закрытое тестирование, beta — открытое
    python tool/release/play.py dist/…aab --track alpha --key play-key.json

    # тексты карточки (store/play/<язык>/) и картинки (store/graphics/)
    python tool/release/play.py --listing --images --key play-key.json

    # только проверить, что ключ работает (ничего не меняет)
    python tool/release/play.py --check --key play-key.json

Ключ — JSON сервисного аккаунта Google Cloud, которому в Play Console выданы
права на приложение (как его получить — docs/RELEASE.md). Вместо --key можно
задать переменную PLAY_SERVICE_ACCOUNT_JSON: путь к файлу или сам JSON.

Первую сборку Google Play требует загрузить вручную в Play Console — до этого
API не знает приложения (ошибка «Package not found»). Пока приложение —
черновик (не прошло первую проверку), выпуск создаётся черновиком: его
нужно открыть в Play Console и нажать «Выпустить».
"""
import argparse
import json
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PACKAGE = 'com.ogneliss.share'
SCOPES = ['https://www.googleapis.com/auth/androidpublisher']
LISTINGS = ROOT / 'store' / 'play'
GRAPHICS = ROOT / 'store' / 'graphics'
SCREENSHOTS = ROOT / 'store' / 'screenshots' / 'play'
WHATS_NEW = ROOT / 'store' / 'whats_new'

# Ограничения Google Play на тексты.
LIMITS = {'title.txt': 30, 'short_description.txt': 80, 'full_description.txt': 4000}
NOTES_LIMIT = 500
# Папки снимков экрана → тип картинки в API.
SHOT_TYPES = {'phone': 'phoneScreenshots', 'tablet7': 'sevenInchScreenshots', 'tablet10': 'tenInchScreenshots'}

for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding='utf-8', errors='replace')
    except (AttributeError, ValueError):
        pass


def log(msg):
    print(f'[play] {msg}', flush=True)


def version():
    m = re.search(r'^version:\s*([0-9.]+)', (ROOT / 'pubspec.yaml').read_text(encoding='utf-8'), re.M)
    return m.group(1) if m else '?'


def read_text(path):
    return path.read_text(encoding='utf-8').strip()


def release_notes(ver):
    """store/whats_new/<версия>.txt в формате Play Console:
    <ru-RU>…</ru-RU> <en-US>…</en-US>."""
    path = WHATS_NEW / f'{ver}.txt'
    if not path.exists():
        log(f'нет {path.relative_to(ROOT)} — выпуск без заметок')
        return []
    notes = []
    for lang, text in re.findall(r'<([A-Za-z]{2,3}(?:-[A-Za-z0-9]+)?)>\s*(.*?)\s*</\1>', read_text(path), re.S):
        if len(text) > NOTES_LIMIT:
            sys.exit(f'{path.name}: заметки {lang} длиннее {NOTES_LIMIT} знаков ({len(text)})')
        notes.append({'language': lang, 'text': text})
    if not notes:
        sys.exit(f'{path.name}: не найдено ни одного блока <язык>…</язык>')
    return notes


def listing_dirs():
    return sorted(p for p in LISTINGS.iterdir() if p.is_dir() and (p / 'title.txt').exists())


def check_listing_texts():
    for d in listing_dirs():
        for name, limit in LIMITS.items():
            text = read_text(d / name)
            if len(text) > limit:
                sys.exit(f'{d.name}/{name}: {len(text)} знаков, а можно не больше {limit}')


def service(key):
    try:
        from google.oauth2 import service_account
        from googleapiclient.discovery import build
    except ImportError:
        sys.exit('Нужны пакеты: pip install google-api-python-client google-auth')
    key = key or os.environ.get('PLAY_SERVICE_ACCOUNT_JSON')
    if not key:
        sys.exit('Нет ключа: --key файл.json или переменная PLAY_SERVICE_ACCOUNT_JSON')
    if key.lstrip().startswith('{'):
        creds = service_account.Credentials.from_service_account_info(json.loads(key), scopes=SCOPES)
    else:
        creds = service_account.Credentials.from_service_account_file(key, scopes=SCOPES)
    return build('androidpublisher', 'v3', credentials=creds, cache_discovery=False)


def explain(err):
    """Понятное объяснение частых ошибок API."""
    text = str(err)
    if 'Package not found' in text or 'applicationNotFound' in text:
        return ('Google Play не знает приложения: первую сборку загрузите вручную в Play Console '
                '(Тестирование → Внутреннее тестирование → Создать выпуск), дальше — этим скриптом.')
    if 'The caller does not have permission' in text or 'permissionDenied' in text or '403' in text:
        return ('Нет прав: в Play Console → Пользователи и разрешения пригласите адрес сервисного аккаунта '
                'и дайте ему права на это приложение (выпуски в тестирование, карточка).')
    if 'version code that has already been used' in text:
        return 'Сборка с таким номером уже загружена — соберите заново (номер растёт сам).'
    return text


def upload_bundle(svc, edit, aab):
    from googleapiclient.http import MediaFileUpload
    media = MediaFileUpload(str(aab), mimetype='application/octet-stream', resumable=True,
                            chunksize=8 * 1024 * 1024)
    request = svc.edits().bundles().upload(packageName=PACKAGE, editId=edit, media_body=media)
    response = None
    while response is None:
        status, response = request.next_chunk()
        if status:
            log(f'загружено {int(status.progress() * 100)}%')
    log(f'сборка загружена, номер {response["versionCode"]}')
    return response['versionCode']


def set_track(svc, edit, track, code, notes, draft):
    from googleapiclient.errors import HttpError
    release = {'name': f'{version()} ({code})', 'versionCodes': [str(code)],
               'status': 'draft' if draft else 'completed'}
    if notes:
        release['releaseNotes'] = notes
    body = {'track': track, 'releases': [release]}
    try:
        svc.edits().tracks().update(packageName=PACKAGE, editId=edit, track=track, body=body).execute()
    except HttpError as err:
        if draft or 'draft app' not in str(err):
            raise
        log('приложение ещё черновик в Play Console — выпуск сохраняется черновиком')
        release['status'] = 'draft'
        svc.edits().tracks().update(packageName=PACKAGE, editId=edit, track=track, body=body).execute()
    return release['status']


def update_listings(svc, edit):
    for d in listing_dirs():
        body = {
            'language': d.name,
            'title': read_text(d / 'title.txt'),
            'shortDescription': read_text(d / 'short_description.txt'),
            'fullDescription': read_text(d / 'full_description.txt'),
        }
        if (d / 'video.txt').exists():
            body['video'] = read_text(d / 'video.txt')
        svc.edits().listings().update(packageName=PACKAGE, editId=edit, language=d.name, body=body).execute()
        log(f'карточка {d.name}: обновлена')


def upload_images(svc, edit):
    from googleapiclient.http import MediaFileUpload
    images = svc.edits().images()

    def replace(lang, kind, files):
        images.deleteall(packageName=PACKAGE, editId=edit, language=lang, imageType=kind).execute()
        for f in files:
            images.upload(packageName=PACKAGE, editId=edit, language=lang, imageType=kind,
                          media_body=MediaFileUpload(str(f), mimetype='image/png')).execute()
        log(f'{lang}: {kind} — {len(files)}')

    for d in listing_dirs():
        lang = d.name
        replace(lang, 'icon', [GRAPHICS / 'play_icon_512.png'])
        replace(lang, 'featureGraphic', [GRAPHICS / 'play_feature_1024x500.png'])
        for folder, kind in SHOT_TYPES.items():
            shots = sorted((SCREENSHOTS / lang / folder).glob('*.png'))
            if shots:
                replace(lang, kind, shots)


def commit(svc, edit):
    from googleapiclient.errors import HttpError
    try:
        svc.edits().commit(packageName=PACKAGE, editId=edit).execute()
    except HttpError as err:
        if 'changesNotSentForReview' not in str(err):
            raise
        # Включено «Управление публикацией» или есть изменения для проверки:
        # отправить их на проверку можно только из Play Console.
        svc.edits().commit(packageName=PACKAGE, editId=edit, changesNotSentForReview=True).execute()
        log('изменения сохранены; на проверку их отправьте в Play Console → «Обзор публикации»')


def main():
    ap = argparse.ArgumentParser(description='Загрузка Ogneliss Share в Google Play')
    ap.add_argument('aab', nargs='?', type=Path, help='файл .aab (tool/release/build.py android)')
    ap.add_argument('--key', help='JSON сервисного аккаунта (или PLAY_SERVICE_ACCOUNT_JSON)')
    ap.add_argument('--track', default='internal',
                    help='internal (по умолчанию), alpha — закрытое, beta — открытое, production')
    ap.add_argument('--draft', action='store_true', help='создать выпуск черновиком')
    ap.add_argument('--listing', action='store_true', help='обновить тексты карточки из store/play/')
    ap.add_argument('--images', action='store_true', help='загрузить значок, баннер и снимки экрана')
    ap.add_argument('--check', action='store_true', help='проверить доступ, ничего не меняя')
    args = ap.parse_args()
    if not (args.aab or args.listing or args.images or args.check):
        ap.error('укажите .aab, --listing, --images или --check')
    if args.aab and not args.aab.exists():
        sys.exit(f'Нет файла {args.aab}')
    if args.listing:
        check_listing_texts()
    notes = release_notes(version()) if args.aab else []

    from googleapiclient.errors import HttpError
    svc = service(args.key)
    try:
        edit = svc.edits().insert(packageName=PACKAGE, body={}).execute()['id']
        if args.check:
            tracks = svc.edits().tracks().list(packageName=PACKAGE, editId=edit).execute().get('tracks', [])
            for t in tracks:
                codes = [c for r in t.get('releases', []) for c in r.get('versionCodes', [])]
                log(f'дорожка {t["track"]}: сборки {", ".join(codes) or "—"}')
            svc.edits().delete(packageName=PACKAGE, editId=edit).execute()
            log('доступ есть, ничего не изменено')
            return
        if args.aab:
            code = upload_bundle(svc, edit, args.aab)
            status = set_track(svc, edit, args.track, code, notes, args.draft)
            log(f'дорожка {args.track}: выпуск {version()} ({code}), статус {status}')
        if args.listing:
            update_listings(svc, edit)
        if args.images:
            upload_images(svc, edit)
        commit(svc, edit)
    except HttpError as err:
        sys.exit(f'Ошибка Google Play: {explain(err)}')
    log('готово')


if __name__ == '__main__':
    main()
