#!/usr/bin/env python3
"""Сборка Ogneliss Share для выпуска: файлы с понятными именами в dist/.

    python tool/release/build.py android          # AAB для Google Play + APK по архитектурам
    python tool/release/build.py android --apk    # только APK (для проверки на телефоне)
    python tool/release/build.py windows          # zip с программой
    python tool/release/build.py windows --msix   # zip + пакет MSIX для Microsoft Store
    python tool/release/build.py linux            # tar.gz
    python tool/release/build.py macos            # zip с .app (без подписи — для проверки)
    python tool/release/build.py ios              # .ipa без подписи (для проверки сборки)

Подписанные сборки для TestFlight (iOS и macOS) делает tool/release/testflight.sh,
загрузку в Google Play — tool/release/play.py. Подробно — docs/RELEASE.md.

Версия берётся из pubspec.yaml. Номер сборки для магазинов (Android, iOS,
macOS) — минуты с 1 января 2026 года по UTC: всегда растёт, годится и для
Google Play, и для App Store. Для Windows и Linux — номер из pubspec.yaml
(в Windows номер файла не больше 65535). Свой номер: --build-number 123.
Каталоги платформ создаются сами (tool/bootstrap.py), если их ещё нет.

Подпись Android: android/key.properties (см. docs/RELEASE.md); без него
сборка подписана отладочным ключом — Google Play такую не примет.
"""
import argparse
import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parents[2]
DIST = ROOT / 'dist'
MSIX_SRC = ROOT / 'tool' / 'release' / 'msix'
IDENTITY = ROOT / 'store' / 'windows' / 'identity.json'

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_apk  # noqa: E402

for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding='utf-8', errors='replace')
    except (AttributeError, ValueError):
        pass


def log(msg):
    print(f'[build] {msg}', flush=True)


def run(cmd, cwd=ROOT, shell=None):
    """Запуск команды. flutter в Windows — это .bat, ему нужна оболочка;
    программам SDK (makeappx и т. п.) — нет: cmd портит кавычки в путях."""
    log(' '.join(str(c) for c in cmd))
    if shell is None:
        shell = os.name == 'nt'
    subprocess.run([str(c) for c in cmd], cwd=cwd, check=True, shell=shell)


def flutter():
    exe = shutil.which('flutter')
    if not exe:
        sys.exit('Не найден flutter в PATH')
    return exe


def pubspec_version():
    """Версия и номер сборки из pubspec.yaml («0.1.1+2» → ('0.1.1', 2))."""
    text = (ROOT / 'pubspec.yaml').read_text(encoding='utf-8')
    m = re.search(r'^version:\s*([0-9]+\.[0-9]+\.[0-9]+)(?:\+([0-9]+))?', text, re.M)
    if not m:
        sys.exit('В pubspec.yaml нет version вида 1.2.3+4')
    return m.group(1), int(m.group(2) or 1)


def store_build_number():
    """Минуты с 01.01.2026 UTC: растёт с каждой сборкой; предел Google Play
    (2 100 000 000) наступит через тысячи лет."""
    start = dt.datetime(2026, 1, 1, tzinfo=dt.timezone.utc)
    return int((dt.datetime.now(dt.timezone.utc) - start).total_seconds() // 60)


def ensure_platform(platform):
    if not (ROOT / platform).is_dir():
        run([sys.executable, ROOT / 'tool' / 'bootstrap.py'], shell=False)


def zip_dir(src, dest, root_name=None):
    with zipfile.ZipFile(dest, 'w', zipfile.ZIP_DEFLATED) as z:
        for p in sorted(src.rglob('*')):
            arc = p.relative_to(src)
            if root_name:
                arc = Path(root_name) / arc
            if p.is_symlink():
                info = zipfile.ZipInfo(str(arc))
                info.external_attr = 0o120777 << 16
                z.writestr(info, os.readlink(p))
            elif p.is_file():
                z.write(p, arc)


# ---------------------------------------------------------------- Android

def build_android(ver, num, args):
    ensure_platform('android')
    if not (ROOT / 'android' / 'key.properties').exists():
        log('⚠ нет android/key.properties — сборка подписана отладочным ключом '
            '(для проверки годится, для Google Play — нет)')
    common = ['--release', f'--build-name={ver}', f'--build-number={num}']
    out = []
    if not args.apk:
        run([flutter(), 'build', 'appbundle', *common])
        aab = DIST / f'ogneliss-share-{ver}-android.aab'
        shutil.copy2(ROOT / 'build/app/outputs/bundle/release/app-release.aab', aab)
        out.append(aab)
    run([flutter(), 'build', 'apk', '--split-per-abi', *common])
    apks = ROOT / 'build/app/outputs/flutter-apk'
    for abi, name in (('arm64-v8a', 'arm64'), ('armeabi-v7a', 'armv7'), ('x86_64', 'x86_64')):
        src = apks / f'app-{abi}-release.apk'
        if src.exists():
            dest = DIST / f'ogneliss-share-{ver}-android-{name}.apk'
            shutil.copy2(src, dest)
            out.append(dest)
    log('проверка страниц 16 КБ (требование Google Play):')
    if sum(check_apk.check(p) for p in out):
        sys.exit('64-битные библиотеки не выровнены на 16 КБ — Google Play сборку не примет')
    return out


# ---------------------------------------------------------------- Windows

def windows_sdk_tool(name):
    """makeappx.exe или makepri.exe из Windows SDK (ставится вместе с Visual
    Studio, нагрузка «Разработка классических приложений на C++»)."""
    candidates = []
    if os.environ.get('WINDOWS_SDK_BIN'):
        candidates.append(Path(os.environ['WINDOWS_SDK_BIN']) / name)
    kits = Path(os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)')) / 'Windows Kits' / '10' / 'bin'
    if kits.is_dir():
        def key(p):
            return tuple(int(x) for x in p.name.split('.') if x.isdigit())
        for v in sorted((p for p in kits.iterdir() if p.is_dir() and p.name[:1].isdigit()), key=key, reverse=True):
            candidates.append(v / 'x64' / name)
        candidates.append(kits / 'x64' / name)
    for c in candidates:
        if c.exists():
            return c
    found = shutil.which(name)
    if found:
        return Path(found)
    sys.exit(f'Не найден {name}: нужен Windows SDK (ставится с Visual Studio, '
             '«Разработка классических приложений на C++»), или укажите папку в WINDOWS_SDK_BIN')


def msix_identity():
    """Имя пакета и издатель — из store/windows/identity.json; переменные
    MSIX_NAME, MSIX_PUBLISHER, MSIX_PUBLISHER_DISPLAY_NAME важнее файла."""
    data = json.loads(IDENTITY.read_text(encoding='utf-8'))
    for key in ('name', 'publisher', 'publisher_display_name'):
        if os.environ.get(f'MSIX_{key.upper()}'):
            data[key] = os.environ[f'MSIX_{key.upper()}']
    if data.get('test'):
        log('⚠ в store/windows/identity.json пробные значения: пакет годится для проверки '
            'на своём компьютере, Partner Center его не примет (см. docs/RELEASE.md)')
    return data


def build_msix(ver, release_dir):
    """Пакет MSIX: программа + значки + манифест; подпись делает Microsoft
    Store при публикации. Версия пакета — X.Y.Z.0 (четвёртая цифра — Store)."""
    ident = msix_identity()
    work = ROOT / 'build' / 'msix'
    layout = work / 'layout'
    if work.exists():
        shutil.rmtree(work)
    shutil.copytree(release_dir, layout)
    shutil.copytree(MSIX_SRC / 'Assets', layout / 'Assets')
    manifest = (MSIX_SRC / 'AppxManifest.xml').read_text(encoding='utf-8')
    values = {
        'IDENTITY_NAME': ident['name'],
        'PUBLISHER': ident['publisher'],
        'PUBLISHER_DISPLAY_NAME': ident['publisher_display_name'],
        'VERSION': f'{ver}.0',
    }
    for k, v in values.items():
        manifest = manifest.replace('{{' + k + '}}', escape(v, {'"': '&quot;'}))
    if '{{' in manifest:
        sys.exit('В AppxManifest.xml остались незаполненные {{…}}')
    (layout / 'AppxManifest.xml').write_text(manifest, encoding='utf-8')

    # resources.pri — по какому файлу брать значок для масштаба и размера.
    # Индексируются только манифест и Assets (в отдельной папке), иначе
    # makepri внёс бы в индекс и все файлы программы.
    pri = work / 'pri'
    pri.mkdir()
    shutil.copy2(layout / 'AppxManifest.xml', pri / 'AppxManifest.xml')
    shutil.copytree(MSIX_SRC / 'Assets', pri / 'Assets')
    makepri = windows_sdk_tool('makepri.exe')
    config = work / 'priconfig.xml'
    run([makepri, 'createconfig', '/cf', config, '/dq', 'en-US', '/o'], shell=False)
    run([makepri, 'new', '/pr', pri, '/cf', config, '/mn', pri / 'AppxManifest.xml',
         '/of', layout / 'resources.pri', '/o'], shell=False)

    dest = DIST / f'ogneliss-share-{ver}-windows-x64.msix'
    run([windows_sdk_tool('makeappx.exe'), 'pack', '/d', layout, '/p', dest, '/o'], shell=False)
    return dest


def build_windows(ver, num, args):
    ensure_platform('windows')
    run([flutter(), 'build', 'windows', '--release', f'--build-name={ver}', f'--build-number={num}'])
    release = ROOT / 'build/windows/x64/runner/Release'
    dest = DIST / f'ogneliss-share-{ver}-windows-x64.zip'
    zip_dir(release, dest, root_name='Ogneliss Share')
    out = [dest]
    if args.msix:
        out.append(build_msix(ver, release))
    return out


# ---------------------------------------------------------------- остальные

def build_linux(ver, num, args):
    ensure_platform('linux')
    run([flutter(), 'build', 'linux', '--release', f'--build-name={ver}', f'--build-number={num}'])
    dest = DIST / f'ogneliss-share-{ver}-linux-x64.tar.gz'
    with tarfile.open(dest, 'w:gz') as t:
        t.add(ROOT / 'build/linux/x64/release/bundle', arcname='ogneliss-share')
    return [dest]


def build_macos(ver, num, args):
    ensure_platform('macos')
    run([flutter(), 'build', 'macos', '--release', f'--build-name={ver}', f'--build-number={num}'])
    app = ROOT / 'build/macos/Build/Products/Release/Ogneliss Share.app'
    dest = DIST / f'ogneliss-share-{ver}-macos.zip'
    # ditto сохраняет атрибуты и ссылки внутри .app — zip из Python их портит.
    run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', app, dest])
    return [dest]


def build_ios(ver, num, args):
    ensure_platform('ios')
    run([flutter(), 'build', 'ios', '--release', '--no-codesign', f'--build-name={ver}', f'--build-number={num}'])
    payload = DIST / 'Payload'
    if payload.exists():
        shutil.rmtree(payload)
    payload.mkdir(parents=True)
    run(['cp', '-R', ROOT / 'build/ios/iphoneos/Runner.app', payload])
    dest = DIST / f'ogneliss-share-{ver}-ios-unsigned.ipa'
    zip_dir(payload, dest, root_name='Payload')
    shutil.rmtree(payload)
    return [dest]


BUILDERS = {
    'android': build_android,
    'windows': build_windows,
    'linux': build_linux,
    'macos': build_macos,
    'ios': build_ios,
}


def main():
    ap = argparse.ArgumentParser(description='Сборка Ogneliss Share в dist/')
    ap.add_argument('platform', choices=sorted(BUILDERS))
    ap.add_argument('--build-number', type=int, default=None,
                    help='номер сборки (по умолчанию: Android, iOS, macOS — минуты с 2026 года; '
                         'Windows, Linux — из pubspec.yaml)')
    ap.add_argument('--apk', action='store_true', help='Android: только APK, без AAB')
    ap.add_argument('--msix', action='store_true', help='Windows: ещё и пакет MSIX для Microsoft Store')
    args = ap.parse_args()
    if args.platform in ('macos', 'ios') and sys.platform != 'darwin':
        sys.exit('Сборка для macOS и iOS — только на Mac с Xcode')
    if args.platform == 'windows' and os.name != 'nt':
        sys.exit('Сборка для Windows — только в Windows')
    ver, pubspec_num = pubspec_version()
    if args.build_number:
        num = args.build_number
    elif args.platform in ('android', 'ios', 'macos'):
        num = store_build_number()
    else:
        num = pubspec_num
    DIST.mkdir(exist_ok=True)
    log(f'Ogneliss Share {ver} ({num}) — {args.platform}')
    files = BUILDERS[args.platform](ver, num, args)
    log('готово:')
    for f in files:
        log(f'  {f.relative_to(ROOT)}  ({f.stat().st_size / 1024 / 1024:.1f} МБ)')


if __name__ == '__main__':
    main()
