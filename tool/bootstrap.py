#!/usr/bin/env python3
"""Подготовить проект Flutter к сборке: каталоги платформ и их настройки.

Каталоги android/ ios/ macos/ windows/ linux/ создаёт `flutter create`
(шаблон той версии Flutter, что установлена), а этот скрипт:
  1) кладёт поверх «накладку» из tool/platform/ — значки, AndroidManifest,
     MainActivity с «Поделиться» и инициализацией ядра;
  2) правит настройки: идентификаторы com.ogneliss.share, названия
     «Ogneliss Share», разрешения камеры и локальной сети, права песочницы
     macOS, подпись Android-сборки ключом из android/key.properties;
  3) выполняет `flutter pub get`.

Запуск из корня репозитория (нужен Flutter в PATH):

    python3 tool/bootstrap.py

Повторный запуск безопасен: существующие файлы `flutter create` не трогает,
а правки применяются только там, где их ещё нет.
"""
import os
import plistlib
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OVERLAY = ROOT / 'tool' / 'platform'

ORG = 'com.ogneliss'
PROJECT = 'ogneliss_share'
APP_ID = 'com.ogneliss.share'
TITLE = 'Ogneliss Share'
AUTHOR = 'Ermishin Oleg'
PLATFORMS = ['android', 'ios', 'macos', 'windows', 'linux']

CAMERA_RU_EN = ('Камера нужна, чтобы отсканировать QR-код другого устройства. '
                '/ The camera is used to scan the QR code of another device.')
LOCAL_NET_RU_EN = ('Ogneliss Share передаёт файлы напрямую между вашими устройствами в локальной сети. '
                   '/ Ogneliss Share sends files directly between your devices on the local network.')


def log(msg):
    print(f'[bootstrap] {msg}', flush=True)


def run(cmd, cwd=ROOT):
    log(' '.join(cmd))
    subprocess.run(cmd, cwd=cwd, check=True, shell=(os.name == 'nt'))


def flutter_bin():
    exe = shutil.which('flutter')
    if not exe:
        sys.exit('Не найден flutter в PATH')
    return exe


def read(path):
    return path.read_text(encoding='utf-8')


def write(path, text):
    path.write_text(text, encoding='utf-8')


def must_sub(text, pattern, repl, path, count=0, flags=0):
    new, n = re.subn(pattern, repl, text, count=count, flags=flags)
    if n == 0:
        sys.exit(f'Не удалось применить правку к {path}: шаблон {pattern!r} не найден '
                 f'(изменился шаблон flutter create?)')
    return new


# ---------------------------------------------------------------- шаги

def create_platforms():
    run([flutter_bin(), 'create', '--org', ORG, '--project-name', PROJECT,
         '--platforms', ','.join(PLATFORMS), '--no-pub', '.'])


def apply_overlay():
    for src in OVERLAY.rglob('*'):
        if src.is_dir():
            continue
        rel = src.relative_to(OVERLAY)
        dest = ROOT / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(src, dest)
    # В наборах значков оставить только наши файлы.
    for iconset in OVERLAY.rglob('*.appiconset'):
        dest = ROOT / iconset.relative_to(OVERLAY)
        ours = {p.name for p in iconset.iterdir()}
        for p in dest.iterdir():
            if p.name not in ours:
                p.unlink()
    log('накладка tool/platform применена')


def patch_android():
    kotlin = ROOT / 'android' / 'app' / 'src' / 'main' / 'kotlin'
    # Шаблонная MainActivity (com/ogneliss/ogneliss_share) не нужна — своя в com/ogneliss/share.
    for f in kotlin.rglob('MainActivity.kt'):
        if f.parent != kotlin / 'com' / 'ogneliss' / 'share':
            f.unlink()
            log(f'удалена шаблонная {f.relative_to(ROOT)}')
    for d in sorted(kotlin.rglob('*'), key=lambda p: len(p.parts), reverse=True):
        if d.is_dir() and not any(d.iterdir()):
            d.rmdir()

    gradle = ROOT / 'android' / 'app' / 'build.gradle.kts'
    text = read(gradle)
    text = re.sub(r'namespace = "[^"]+"', f'namespace = "{APP_ID}"', text)
    text = re.sub(r'applicationId = "[^"]+"', f'applicationId = "{APP_ID}"', text)
    if 'maxOf(flutter.minSdkVersion' not in text:
        text = must_sub(text, r'minSdk = flutter\.minSdkVersion', 'minSdk = maxOf(flutter.minSdkVersion, 24)', gradle)
    if 'keystoreProperties' not in text:
        signing_props = (
            '\n// Ключ подписи выпускаемой сборки — android/key.properties (в git не попадает):\n'
            '//   storeFile=…  storePassword=…  keyAlias=…  keyPassword=…\n'
            '// Нет файла — сборка подписывается отладочным ключом (для проверки, не для магазинов).\n'
            'val keystoreProperties = java.util.Properties().apply {\n'
            '    val f = rootProject.file("key.properties")\n'
            '    if (f.exists()) f.inputStream().use { load(it) }\n'
            '}\n'
            'val hasReleaseKey = keystoreProperties.getProperty("storeFile") != null\n'
        )
        # После блока plugins { … }.
        text = must_sub(text, r'(plugins \{.*?\n\}\n)', lambda m: m.group(1) + signing_props, gradle,
                        count=1, flags=re.S)
        signing_configs = (
            '    signingConfigs {\n'
            '        if (hasReleaseKey) {\n'
            '            create("release") {\n'
            '                storeFile = file(keystoreProperties.getProperty("storeFile"))\n'
            '                storePassword = keystoreProperties.getProperty("storePassword")\n'
            '                keyAlias = keystoreProperties.getProperty("keyAlias")\n'
            '                keyPassword = keystoreProperties.getProperty("keyPassword")\n'
            '            }\n'
            '        }\n'
            '    }\n\n'
            '    buildTypes {'
        )
        text = must_sub(text, r'    buildTypes \{', signing_configs, gradle, count=1)
        text = must_sub(
            text,
            r'signingConfig = signingConfigs\.getByName\("debug"\)',
            'signingConfig = if (hasReleaseKey) signingConfigs.getByName("release") '
            'else signingConfigs.getByName("debug")',
            gradle, count=1)
    write(gradle, text)

    props = ROOT / 'android' / 'gradle.properties'
    text = read(props)
    for line in ('android.useAndroidX=true', 'android.newDsl=false', 'android.builtInKotlin=false'):
        key = line.split('=')[0]
        if not re.search(rf'^{re.escape(key)}=', text, flags=re.M):
            text = text.rstrip('\n') + f'\n{line}\n'
    write(props, text)
    log('android: com.ogneliss.share, minSdk 24, подпись из key.properties')


def patch_plist(path, updates):
    with open(path, 'rb') as f:
        data = plistlib.load(f)
    data.update(updates)
    with open(path, 'wb') as f:
        plistlib.dump(data, f)


def patch_bundle_ids(pbxproj):
    text = read(pbxproj)

    def repl(m):
        value = m.group(1)
        return f'PRODUCT_BUNDLE_IDENTIFIER = {APP_ID}.RunnerTests;' if value.endswith('.RunnerTests') \
            else f'PRODUCT_BUNDLE_IDENTIFIER = {APP_ID};'

    text = re.sub(r'PRODUCT_BUNDLE_IDENTIFIER = ([^;\n]+);', repl, text)
    write(pbxproj, text)


def patch_ios():
    patch_plist(ROOT / 'ios' / 'Runner' / 'Info.plist', {
        'CFBundleDisplayName': TITLE,
        'CFBundleName': TITLE,
        'CFBundleLocalizations': ['ru', 'en'],
        'NSCameraUsageDescription': CAMERA_RU_EN,
        'NSLocalNetworkUsageDescription': LOCAL_NET_RU_EN,
        # Полученные файлы видны в «Файлах» → «На iPhone» → Ogneliss Share.
        'UIFileSharingEnabled': True,
        'LSSupportsOpeningDocumentsInPlace': True,
        # Шифрование — только стандартное (TLS 1.3/QUIC): экспортное освобождение.
        'ITSAppUsesNonExemptEncryption': False,
    })
    patch_bundle_ids(ROOT / 'ios' / 'Runner.xcodeproj' / 'project.pbxproj')
    log('ios: Info.plist и идентификатор')


def patch_macos():
    runner = ROOT / 'macos' / 'Runner'
    patch_plist(runner / 'Info.plist', {
        'NSCameraUsageDescription': CAMERA_RU_EN,
        'CFBundleLocalizations': ['ru', 'en'],
        'LSApplicationCategoryType': 'public.app-category.utilities',
    })
    entitlements = {
        'com.apple.security.app-sandbox': True,
        'com.apple.security.network.client': True,
        'com.apple.security.network.server': True,
        'com.apple.security.files.user-selected.read-write': True,
        'com.apple.security.files.downloads.read-write': True,
        'com.apple.security.device.camera': True,
    }
    for name in ('DebugProfile.entitlements', 'Release.entitlements'):
        patch_plist(runner / name, entitlements)
    info = runner / 'Configs' / 'AppInfo.xcconfig'
    text = read(info)
    text = re.sub(r'^PRODUCT_NAME = .*$', f'PRODUCT_NAME = {TITLE}', text, flags=re.M)
    text = re.sub(r'^PRODUCT_BUNDLE_IDENTIFIER = .*$', f'PRODUCT_BUNDLE_IDENTIFIER = {APP_ID}', text, flags=re.M)
    text = re.sub(r'^PRODUCT_COPYRIGHT = .*$',
                  f'PRODUCT_COPYRIGHT = Copyright © 2026 {AUTHOR}. Apache License 2.0', text, flags=re.M)
    write(info, text)
    patch_bundle_ids(ROOT / 'macos' / 'Runner.xcodeproj' / 'project.pbxproj')
    log('macos: права песочницы, камера, название')


def patch_windows():
    main_cpp = ROOT / 'windows' / 'runner' / 'main.cpp'
    text = read(main_cpp)
    text = re.sub(r'window\.Create\(L"[^"]*"', f'window.Create(L"{TITLE}"', text)
    write(main_cpp, text)
    rc = ROOT / 'windows' / 'runner' / 'Runner.rc'
    text = read(rc)
    for key, value in (('CompanyName', AUTHOR), ('FileDescription', TITLE), ('ProductName', TITLE),
                       ('LegalCopyright', f'Copyright (C) 2026 {AUTHOR}. Apache License 2.0')):
        text = re.sub(rf'VALUE "{key}", "[^"]*"', f'VALUE "{key}", "{value}"', text)
    write(rc, text)
    log('windows: заголовок окна и сведения о файле')


def patch_linux():
    cmake = ROOT / 'linux' / 'CMakeLists.txt'
    text = read(cmake)
    text = re.sub(r'set\(APPLICATION_ID "[^"]*"\)', f'set(APPLICATION_ID "{APP_ID}")', text)
    write(cmake, text)
    app = ROOT / 'linux' / 'runner' / 'my_application.cc'
    if not app.exists():
        app = ROOT / 'linux' / 'my_application.cc'
    text = read(app)
    text = text.replace(f'"{PROJECT}"', f'"{TITLE}"')
    write(app, text)
    log('linux: идентификатор и заголовок окна')


def main():
    os.chdir(ROOT)
    create_platforms()
    apply_overlay()
    patch_android()
    patch_ios()
    patch_macos()
    patch_windows()
    patch_linux()
    if '--no-pub' not in sys.argv:
        run([flutter_bin(), 'pub', 'get'])
    log('готово')


if __name__ == '__main__':
    main()
