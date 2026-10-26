#!/usr/bin/env python3
"""Подготовить проект Flutter к сборке: каталоги платформ и их настройки.

Каталоги android/ ios/ macos/ windows/ linux/ создаёт `flutter create`
(шаблон той версии Flutter, что установлена), а этот скрипт:
  1) кладёт поверх «накладку» из tool/platform/ — значки, AndroidManifest,
     MainActivity с «Поделиться» и инициализацией ядра, строки Android;
  2) правит настройки: идентификаторы com.ogneliss.share, названия
     «Ogneliss Share», разрешения камеры и локальной сети, права песочницы
     macOS, манифест приватности iOS и macOS (PrivacyInfo.xcprivacy),
     подпись Android-сборки ключом из android/key.properties;
  3) переводит системные тексты: запросы разрешений iOS и macOS берутся из
     lib/l10n/app_*.arb (InfoPlist.strings для en и ru), меню macOS — из
     tool/l10n/macos_menu.json;
  4) выполняет `flutter pub get` и `flutter gen-l10n`.

Запуск из корня репозитория (нужен Flutter в PATH):

    python3 tool/bootstrap.py

Повторный запуск безопасен: существующие файлы `flutter create` не трогает,
а правки применяются только там, где их ещё нет.
"""
import hashlib
import json
import os
import plistlib
import re
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OVERLAY = ROOT / 'tool' / 'platform'
L10N = ROOT / 'lib' / 'l10n'
MAC_MENU = ROOT / 'tool' / 'l10n' / 'macos_menu.json'

ORG = 'com.ogneliss'
PROJECT = 'ogneliss_share'
APP_ID = 'com.ogneliss.share'
TITLE = 'Ogneliss Share'
AUTHOR = 'Ermishin Oleg'
PLATFORMS = ['android', 'ios', 'macos', 'windows', 'linux']
LANGS = ['en', 'ru']

# Тексты системных запросов: ключ Info.plist → ключ в lib/l10n/app_*.arb.
PERMISSION_TEXTS = {
    'NSCameraUsageDescription': 'permissionCamera',
    # iOS 14+ и macOS 15+ спрашивают разрешение на локальную сеть.
    'NSLocalNetworkUsageDescription': 'permissionLocalNetwork',
}

# Консоль Windows по умолчанию не в UTF-8 — русские сообщения ломали запуск.
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding='utf-8', errors='replace')
    except (AttributeError, ValueError):
        pass


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


def arb(lang):
    return json.loads(read(L10N / f'app_{lang}.arb'))


# ---------------------------------------------------------------- Xcode

def pbx_id(name):
    """Постоянный идентификатор объекта Xcode — одинаковый при каждом запуске."""
    return hashlib.sha1(f'ogneliss-share/{name}'.encode()).hexdigest()[:24].upper()


def pbx_find(text, comment, isa, path):
    """Идентификатор объекта «ID /* comment */ = { isa = …; »."""
    m = re.search(rf'\t\t([0-9A-F]{{24}}) /\* {re.escape(comment)} \*/ = \{{\n\t\t\tisa = {isa};', text)
    if not m:
        sys.exit(f'В {path} не найден объект {comment} ({isa}) — изменился шаблон flutter create?')
    return m.group(1)


def pbx_block(text, obj_id, path):
    """Начало и конец описания объекта (не ссылок на него из списков)."""
    m = re.search(rf'\n\t\t{obj_id} (?:/\*[^\n]*?\*/ )?= \{{', text)
    if not m:
        sys.exit(f'В {path} нет объекта {obj_id}')
    return m.start() + 1, text.index('\n\t\t};', m.start() + 1)


def pbx_add_to_list(text, obj_id, key, item, path):
    """Добавить элемент в список «key = ( … );» объекта obj_id, если его там нет."""
    start, end = pbx_block(text, obj_id, path)
    block = text[start:end]
    m = re.search(rf'\n(\t+){key} = \(\n(.*?)\1\);', block, flags=re.S)
    if not m:
        sys.exit(f'В {path} у объекта {obj_id} нет списка {key}')
    items = [line.strip().rstrip(',') for line in m.group(2).splitlines()]
    if item in items:
        return text
    block = block[:m.end(2)] + f'{m.group(1)}\t{item},\n' + block[m.end(2):]
    return text[:start] + block + text[end:]


def pbx_add_to_section(text, section, entry, path):
    end = f'/* End {section} section */'
    if end not in text:
        sys.exit(f'В {path} нет раздела {section}')
    return text.replace(end, entry + end, 1)


def pbx_resources_phase(text, path):
    """Шаг «Copy Bundle Resources» приложения (не тестов)."""
    target = pbx_find(text, 'Runner', 'PBXNativeTarget', path)
    start, end = pbx_block(text, target, path)
    m = re.search(r'([0-9A-F]{24}) /\* Resources \*/', text[start:end])
    if not m:
        sys.exit(f'В {path} у цели Runner нет шага Resources')
    return m.group(1)


def pbx_file_ref(ref, lang, file_path):
    return (f'\t\t{ref} /* {lang} */ = {{isa = PBXFileReference; lastKnownFileType = text.plist.strings; '
            f'name = {lang}; path = {file_path}; sourceTree = "<group>"; }};\n')


def pbx_add_regions(text, langs, path):
    project = pbx_find(text, 'Project object', 'PBXProject', path)
    for lang in langs:
        text = pbx_add_to_list(text, project, 'knownRegions', lang, path)
    return text


def pbx_add_localized(pbxproj, name, langs):
    """Локализованный ресурс Runner/<язык>.lproj/<name> — в группу Runner
    и в ресурсы приложения (группа вариантов, как делает Xcode)."""
    text = read(pbxproj)
    group = pbx_id(f'{name}/variants')
    if group not in text:
        build = pbx_id(f'{name}/build')
        refs = [(lang, pbx_id(f'{name}/{lang}')) for lang in langs]
        text = pbx_add_to_section(
            text, 'PBXBuildFile',
            f'\t\t{build} /* {name} in Resources */ = {{isa = PBXBuildFile; fileRef = {group} /* {name} */; }};\n',
            pbxproj)
        for lang, ref in refs:
            text = pbx_add_to_section(text, 'PBXFileReference', pbx_file_ref(ref, lang, f'{lang}.lproj/{name}'),
                                      pbxproj)
        children = ''.join(f'\t\t\t\t{ref} /* {lang} */,\n' for lang, ref in refs)
        text = pbx_add_to_section(
            text, 'PBXVariantGroup',
            f'\t\t{group} /* {name} */ = {{\n'
            f'\t\t\tisa = PBXVariantGroup;\n'
            f'\t\t\tchildren = (\n{children}\t\t\t);\n'
            f'\t\t\tname = {name};\n'
            f'\t\t\tsourceTree = "<group>";\n'
            f'\t\t}};\n',
            pbxproj)
        runner = pbx_find(text, 'Runner', 'PBXGroup', pbxproj)
        text = pbx_add_to_list(text, runner, 'children', f'{group} /* {name} */', pbxproj)
        phase = pbx_resources_phase(text, pbxproj)
        text = pbx_add_to_list(text, phase, 'files', f'{build} /* {name} in Resources */', pbxproj)
    text = pbx_add_regions(text, langs, pbxproj)
    write(pbxproj, text)


def strings_file(entries):
    """Файл .strings (UTF-8 Xcode принимает и сам переводит в нужный вид)."""
    def q(s):
        return s.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n')
    lines = ['/* Создано tool/bootstrap.py — не править: источник в lib/l10n и tool/l10n. */', '']
    lines += [f'"{q(k)}" = "{q(v)}";' for k, v in entries.items()]
    return '\n'.join(lines) + '\n'


def pbx_add_resource(pbxproj, name, file_type):
    """Обычный ресурс Runner/<name> — в группу Runner и в ресурсы приложения."""
    text = read(pbxproj)
    ref = pbx_id(f'{name}/ref')
    if ref in text:
        return
    build = pbx_id(f'{name}/build')
    text = pbx_add_to_section(
        text, 'PBXBuildFile',
        f'\t\t{build} /* {name} in Resources */ = {{isa = PBXBuildFile; fileRef = {ref} /* {name} */; }};\n',
        pbxproj)
    text = pbx_add_to_section(
        text, 'PBXFileReference',
        f'\t\t{ref} /* {name} */ = {{isa = PBXFileReference; lastKnownFileType = {file_type}; '
        f'path = {name}; sourceTree = "<group>"; }};\n',
        pbxproj)
    runner = pbx_find(text, 'Runner', 'PBXGroup', pbxproj)
    text = pbx_add_to_list(text, runner, 'children', f'{ref} /* {name} */', pbxproj)
    phase = pbx_resources_phase(text, pbxproj)
    text = pbx_add_to_list(text, phase, 'files', f'{build} /* {name} in Resources */', pbxproj)
    write(pbxproj, text)


# Манифест приватности (App Store требует его с 2024 года). Сбора данных и
# слежения нет; объявлены «API с обоснованием», которые вызывает ядро:
#   · время и размер файлов (stat) — свои файлы приложения (C617.1) и файлы,
#     которые выбрал человек (3B52.1);
#   · свободное место на диске перед приёмом (E174.1);
#   · время с загрузки системы — таймеры и измерение интервалов (35F9.1).
PRIVACY_MANIFEST = {
    'NSPrivacyTracking': False,
    'NSPrivacyTrackingDomains': [],
    'NSPrivacyCollectedDataTypes': [],
    'NSPrivacyAccessedAPITypes': [
        {'NSPrivacyAccessedAPIType': 'NSPrivacyAccessedAPICategoryFileTimestamp',
         'NSPrivacyAccessedAPITypeReasons': ['C617.1', '3B52.1']},
        {'NSPrivacyAccessedAPIType': 'NSPrivacyAccessedAPICategoryDiskSpace',
         'NSPrivacyAccessedAPITypeReasons': ['E174.1']},
        {'NSPrivacyAccessedAPIType': 'NSPrivacyAccessedAPICategorySystemBootTime',
         'NSPrivacyAccessedAPITypeReasons': ['35F9.1']},
    ],
}


def add_privacy_manifest(runner, pbxproj):
    with open(runner / 'PrivacyInfo.xcprivacy', 'wb') as f:
        plistlib.dump(PRIVACY_MANIFEST, f)
    pbx_add_resource(pbxproj, 'PrivacyInfo.xcprivacy', 'text.xml')


# Шифрование: стандартные алгоритмы (TLS 1.3, QUIC) из своей библиотеки, а не
# из iOS. По правилам App Store документы для этого нужны только при выпуске
# в App Store Франции (декларация ANSSI), поэтому NO — пока Франции нет в
# списке стран. Подробно — docs/RELEASE.md, «Экспортные правила».
ENCRYPTION_KEYS = {'ITSAppUsesNonExemptEncryption': False}


def localize_info_plist(runner, pbxproj, keys):
    """Запросы разрешений на языке системы: <язык>.lproj/InfoPlist.strings."""
    for lang in LANGS:
        texts = arb(lang)
        folder = runner / f'{lang}.lproj'
        folder.mkdir(exist_ok=True)
        write(folder / 'InfoPlist.strings', strings_file({k: texts[v] for k, v in keys.items()}))
    pbx_add_localized(pbxproj, 'InfoPlist.strings', LANGS)


def localize_mac_menu(runner, pbxproj):
    """Главное меню macOS по-русски: ru.lproj/MainMenu.strings к MainMenu.xib."""
    xib = runner / 'Base.lproj' / 'MainMenu.xib'
    table = json.loads(read(MAC_MENU))['ru']
    entries = {}
    for el in ET.parse(xib).iter():
        title, oid = el.get('title'), el.get('id')
        if el.tag in ('menu', 'menuItem') and title in table and oid:
            entries[f'{oid}.title'] = table[title]
    if not entries:
        sys.exit(f'В {xib} не нашлось пунктов меню для перевода')
    folder = runner / 'ru.lproj'
    folder.mkdir(exist_ok=True)
    write(folder / 'MainMenu.strings', strings_file(entries))

    text = read(pbxproj)
    ref = pbx_id('MainMenu.strings/ru')
    if ref not in text:
        text = pbx_add_to_section(text, 'PBXFileReference', pbx_file_ref(ref, 'ru', 'ru.lproj/MainMenu.strings'),
                                  pbxproj)
        menu = pbx_find(text, 'MainMenu.xib', 'PBXVariantGroup', pbxproj)
        text = pbx_add_to_list(text, menu, 'children', f'{ref} /* ru */', pbxproj)
    text = pbx_add_regions(text, ['ru'], pbxproj)
    write(pbxproj, text)


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
    # В скриптах Gradle на Kotlin «java» — это расширение проекта, а не пакет:
    # класс Properties нужно импортировать (так было в прежних версиях скрипта).
    text = text.replace('java.util.Properties()', 'Properties()')
    if 'keystoreProperties' not in text:
        signing_props = (
            '\n// Ключ подписи выпускаемой сборки — android/key.properties (в git не попадает):\n'
            '//   storeFile=…  storePassword=…  keyAlias=…  keyPassword=…\n'
            '// Нет файла — сборка подписывается отладочным ключом (для проверки, не для магазинов).\n'
            'val keystoreProperties = Properties()\n'
            'val keystorePropertiesFile = rootProject.file("key.properties")\n'
            'if (keystorePropertiesFile.exists()) {\n'
            '    keystorePropertiesFile.inputStream().use { keystoreProperties.load(it) }\n'
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
    if 'import java.util.Properties' not in text:
        text = 'import java.util.Properties\n\n' + text
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


IOS_NO_BACKUP = '''\
    // Ключ устройства и недокачанные файлы не попадают в резервные копии
    // (iCloud, компьютер): копия, восстановленная на другом iPhone, не должна
    // выдавать себя за это устройство. Полученные файлы копируются как обычно.
    let fileManager = FileManager.default
    var noBackup = fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask)
    if let documents = fileManager.urls(for: .documentDirectory, in: .userDomainMask).first {
      noBackup.append(documents.appendingPathComponent(".ogneliss-share", isDirectory: true))
    }
    for directory in noBackup {
      var url = directory
      try? fileManager.createDirectory(at: url, withIntermediateDirectories: true)
      var values = URLResourceValues()
      values.isExcludedFromBackup = true
      try? url.setResourceValues(values)
    }
'''


def patch_ios():
    en = arb('en')
    runner = ROOT / 'ios' / 'Runner'
    pbxproj = ROOT / 'ios' / 'Runner.xcodeproj' / 'project.pbxproj'
    patch_plist(runner / 'Info.plist', {
        'CFBundleDisplayName': TITLE,
        'CFBundleName': TITLE,
        'CFBundleLocalizations': LANGS,
        # Английский — по умолчанию; переводы — в <язык>.lproj/InfoPlist.strings.
        **{key: en[arb_key] for key, arb_key in PERMISSION_TEXTS.items()},
        # Полученные файлы видны в «Файлах» → «На iPhone» → Ogneliss Share.
        'UIFileSharingEnabled': True,
        'LSSupportsOpeningDocumentsInPlace': True,
        **ENCRYPTION_KEYS,
    })
    patch_bundle_ids(pbxproj)
    localize_info_plist(runner, pbxproj, PERMISSION_TEXTS)
    add_privacy_manifest(runner, pbxproj)

    delegate = runner / 'AppDelegate.swift'
    text = read(delegate)
    if 'isExcludedFromBackup' not in text:
        text = must_sub(text, r'(\n)(    return super\.application\(application, didFinishLaunchingWithOptions)',
                        lambda m: m.group(1) + IOS_NO_BACKUP + m.group(2), delegate, count=1)
        write(delegate, text)
    log('ios: Info.plist, переводы, идентификатор, манифест приватности, без резервных копий ключа')


def patch_macos():
    en = arb('en')
    runner = ROOT / 'macos' / 'Runner'
    pbxproj = ROOT / 'macos' / 'Runner.xcodeproj' / 'project.pbxproj'
    patch_plist(runner / 'Info.plist', {
        'CFBundleLocalizations': LANGS,
        **{key: en[arb_key] for key, arb_key in PERMISSION_TEXTS.items()},
        'LSApplicationCategoryType': 'public.app-category.utilities',
        **ENCRYPTION_KEYS,
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
    patch_bundle_ids(pbxproj)
    localize_info_plist(runner, pbxproj, PERMISSION_TEXTS)
    localize_mac_menu(runner, pbxproj)
    add_privacy_manifest(runner, pbxproj)
    log('macos: права песочницы, камера, название, переводы меню и запросов, манифест приватности')


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
        # Классы переводов (lib/l10n/app_localizations*.dart) в git не хранятся.
        run([flutter_bin(), 'gen-l10n'])
    log('готово')


if __name__ == '__main__':
    main()
