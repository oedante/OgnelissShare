#!/usr/bin/env python3
"""Значки приложения для всех платформ из assets/icons (их рисует make_icon.py).

Результат кладётся в tool/platform/ — это «накладка» поверх каталогов,
которые создаёт `flutter create`; tool/bootstrap.py копирует её на место.

    python3 tool/branding/make_platform_icons.py      # нужен Pillow
"""
import json
import shutil
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
ICONS = ROOT / 'assets' / 'icons'
OUT = ROOT / 'tool' / 'platform'

SLATE = '#3A4450'


def save(img, path, size, mode='RGBA'):
    path.parent.mkdir(parents=True, exist_ok=True)
    out = img.resize((size, size), Image.LANCZOS)
    if mode == 'RGB':
        out = out.convert('RGB')
    out.save(path, optimize=True)


def android():
    res = OUT / 'android' / 'app' / 'src' / 'main' / 'res'
    for old in list(res.glob('mipmap-*')) + list(res.glob('drawable-*')):
        shutil.rmtree(old)
    rounded = Image.open(ICONS / 'app_icon_rounded.png').convert('RGBA')
    fg = Image.open(ICONS / 'app_icon_foreground.png').convert('RGBA')
    mono = Image.open(ICONS / 'app_icon_monochrome.png').convert('RGBA')
    # Обычный значок (Android 7) — 48 dp; слои адаптивного — 108 dp.
    for density, scale in (('mdpi', 1), ('hdpi', 1.5), ('xhdpi', 2), ('xxhdpi', 3), ('xxxhdpi', 4)):
        save(rounded, res / f'mipmap-{density}' / 'ic_launcher.png', round(48 * scale))
        save(fg, res / f'drawable-{density}' / 'ic_launcher_foreground.png', round(108 * scale))
        save(mono, res / f'drawable-{density}' / 'ic_launcher_monochrome.png', round(108 * scale))
    (res / 'mipmap-anydpi-v26').mkdir(parents=True, exist_ok=True)
    (res / 'mipmap-anydpi-v26' / 'ic_launcher.xml').write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<!-- Адаптивный значок: сланцевый фон, знак и одноцветный слой для\n'
        '     тематических значков Android 13+. -->\n'
        '<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
        '    <background android:drawable="@color/ic_launcher_background"/>\n'
        '    <foreground android:drawable="@drawable/ic_launcher_foreground"/>\n'
        '    <monochrome android:drawable="@drawable/ic_launcher_monochrome"/>\n'
        '</adaptive-icon>\n', encoding='utf-8')
    (res / 'values').mkdir(parents=True, exist_ok=True)
    (res / 'values' / 'ic_launcher_background.xml').write_text(
        '<?xml version="1.0" encoding="utf-8"?>\n'
        '<resources>\n'
        f'    <color name="ic_launcher_background">{SLATE}</color>\n'
        '</resources>\n', encoding='utf-8')


def ios():
    d = OUT / 'ios' / 'Runner' / 'Assets.xcassets' / 'AppIcon.appiconset'
    if d.exists():
        shutil.rmtree(d)
    full = Image.open(ICONS / 'app_icon.png').convert('RGBA')
    # App Store не принимает значок с прозрачностью — только RGB.
    save(full, d / 'icon-1024.png', 1024, mode='RGB')
    (d / 'Contents.json').write_text(json.dumps({
        'images': [{
            'filename': 'icon-1024.png',
            'idiom': 'universal',
            'platform': 'ios',
            'size': '1024x1024',
        }],
        'info': {'author': 'xcode', 'version': 1},
    }, indent=2) + '\n', encoding='utf-8')


def macos():
    d = OUT / 'macos' / 'Runner' / 'Assets.xcassets' / 'AppIcon.appiconset'
    if d.exists():
        shutil.rmtree(d)
    mac = Image.open(ICONS / 'app_icon_macos.png').convert('RGBA')
    images = []
    for pt in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            px = pt * scale
            name = f'app_icon_{px}.png'
            if not (d / name).exists():
                save(mac, d / name, px)
            images.append({'size': f'{pt}x{pt}', 'idiom': 'mac', 'filename': name, 'scale': f'{scale}x'})
    (d / 'Contents.json').write_text(json.dumps(
        {'images': images, 'info': {'version': 1, 'author': 'xcode'}}, indent=2) + '\n', encoding='utf-8')


def windows():
    path = OUT / 'windows' / 'runner' / 'resources' / 'app_icon.ico'
    path.parent.mkdir(parents=True, exist_ok=True)
    rounded = Image.open(ICONS / 'app_icon_rounded.png').convert('RGBA')
    rounded.resize((256, 256), Image.LANCZOS).save(
        path, sizes=[(16, 16), (20, 20), (24, 24), (32, 32), (40, 40), (48, 48), (64, 64), (128, 128), (256, 256)])


def linux():
    # Значок окна и ярлыка (.desktop) для Linux.
    rounded = Image.open(ICONS / 'app_icon_rounded.png').convert('RGBA')
    save(rounded, OUT / 'linux' / 'runner' / 'resources' / 'app_icon.png', 256)


def msix():
    """Значки пакета MSIX (Microsoft Store): tool/release/msix/Assets.

    Имена с уточнениями (.scale-200, .targetsize-32…) Windows выбирает по
    resources.pri — его делает tool/release/build.py (makepri); файлы без
    уточнений — запасные, на случай пакета без resources.pri.
    altform-unplated — значок без цветной подложки (панель задач, «Пуск»):
    у нас своя скруглённая плитка, подложка не нужна.
    """
    d = ROOT / 'tool' / 'release' / 'msix' / 'Assets'
    if d.exists():
        shutil.rmtree(d)
    d.mkdir(parents=True)
    rounded = Image.open(ICONS / 'app_icon_rounded.png').convert('RGBA')

    def padded(px, frac):
        """Значок с полями (плитка «Пуск» 150×150: знак не во всю плитку)."""
        canvas = Image.new('RGBA', (px, px), (0, 0, 0, 0))
        inner = rounded.resize((round(px * frac), round(px * frac)), Image.LANCZOS)
        canvas.alpha_composite(inner, ((px - inner.width) // 2, (px - inner.height) // 2))
        return canvas

    scales = (100, 125, 150, 200, 400)
    for name, base, frac in (('Square44x44Logo', 44, 1.0), ('Square150x150Logo', 150, 0.66), ('StoreLogo', 50, 1.0)):
        padded(base, frac).save(d / f'{name}.png', optimize=True)
        for s in scales:
            padded(round(base * s / 100), frac).save(d / f'{name}.scale-{s}.png', optimize=True)
    for t in (16, 24, 32, 48, 256):
        img = rounded.resize((t, t), Image.LANCZOS)
        img.save(d / f'Square44x44Logo.targetsize-{t}.png', optimize=True)
        img.save(d / f'Square44x44Logo.targetsize-{t}_altform-unplated.png', optimize=True)


if __name__ == '__main__':
    android()
    ios()
    macos()
    windows()
    linux()
    msix()
    print('ok')
