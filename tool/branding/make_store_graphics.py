#!/usr/bin/env python3
"""Картинки для магазинов: значок и баннер Google Play.

    python3 tool/branding/make_store_graphics.py     # нужны Pillow и cairosvg

Что пишется:
  store/graphics/play_icon_512.png         — значок 512×512 (Google Play сам
                                             скругляет углы, поэтому квадрат)
  store/graphics/play_feature_1024x500.png — баннер (feature graphic)

Баннер без слов на каком-либо языке — как у Ogneliss Dialer: баннер в
Google Play один на все языки карточки. Название — имя, оно не переводится.
Под ним — «телефон ⇄ ноутбук ⇄ компьютер»: те же фигуры, что рисует
приложение для устройств в списке (Material Icons, Apache 2.0, Google).

App Store Connect значок отдельно не просит: он берётся из сборки
(ios/Runner/Assets.xcassets, его рисует make_platform_icons.py).
"""
import io
from pathlib import Path

import cairosvg
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[2]
ICONS = ROOT / 'assets' / 'icons'
BRAND = ROOT / 'assets' / 'brand'
OUT = ROOT / 'store' / 'graphics'

W, H = 1024, 500
SLATE_TOP = (0x46, 0x50, 0x5C)
SLATE = (0x3A, 0x44, 0x50)
LIGHT = (0xEE, 0xF1, 0xF4)
MUTED = (0xB4, 0xBC, 0xC6)
GOLD = (0xCC, 0xAA, 0x6D)

# Material Icons (Apache 2.0): phone_android, laptop_mac, desktop_windows,
# swap_horiz — github.com/google/material-design-icons, src/*/materialicons
# (компьютер — materialiconsoutlined: с экраном, как телефон и ноутбук).
PHONE = ('M16 1H8C6.34 1 5 2.34 5 4v16c0 1.66 1.34 3 3 3h8c1.66 0 3-1.34 3-3V4c0-1.66-1.34-3-3-3zm-2 20h-4v-1h4v1z'
         'm3.25-3H6.75V4h10.5v14z')
LAPTOP = ('M20 18c1.1 0 1.99-.9 1.99-2L22 5c0-1.1-.9-2-2-2H4c-1.1 0-2 .9-2 2v11c0 1.1.9 2 2 2H0c0 1.1.9 2 2 2h20'
          'c1.1 0 2-.9 2-2h-4zM4 5h16v11H4V5zm8 14c-.55 0-1-.45-1-1s.45-1 1-1 1 .45 1 1-.45 1-1 1z')
DESKTOP = 'M21 2H3c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h7v2H8v2h8v-2h-2v-2h7c1.1 0 2-.9 2-2V4c0-1.1-.9-2-2-2zm0 14H3V4h18v12z'
SWAP = 'M6.99 11L3 15l3.99 4v-3H14v-2H6.99v-3zM21 9l-3.99-4v3H10v2h7.01v3L21 9z'

FONTS = (
    '/usr/share/fonts/opentype/inter/Inter-SemiBold.otf',
    '/usr/share/fonts/truetype/inter/Inter-SemiBold.ttf',
    '/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf',
)


def font(size):
    for path in FONTS:
        if Path(path).exists():
            return ImageFont.truetype(path, size)
    raise SystemExit('Нет шрифта Inter или DejaVu — установите fonts-inter')


def pictogram(d, height, color):
    """Фигура по своим настоящим границам, приведённая к общей высоте.
    Рисуется вчетверо крупнее и уменьшается — края без зазубрин."""
    big = height * 8
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="{big}" height="{big}">'
           f'<path d="{d}" fill="#%02x%02x%02x"/></svg>' % color)
    img = Image.open(io.BytesIO(cairosvg.svg2png(bytestring=svg.encode()))).convert('RGBA')
    img = img.crop(img.getbbox())
    width = max(1, round(img.width * height / img.height))
    return img.resize((width, height), Image.LANCZOS)


def background():
    img = Image.new('RGB', (W, H), SLATE)
    top = Image.new('RGB', (W, H), SLATE_TOP)
    ramp = Image.linear_gradient('L').resize((W, H)).point(lambda v: 255 - v)
    img = Image.composite(top, img, ramp)
    # Мягкий свет за знаком — фон не плоский.
    glow = Image.new('L', (W, H), 0)
    ImageDraw.Draw(glow).ellipse((40, 30, 470, 470), fill=40)
    light = Image.new('RGB', (W, H), (255, 255, 255))
    return Image.composite(light, img, glow.filter(ImageFilter.GaussianBlur(70)))


def feature_graphic():
    img = background().convert('RGBA')

    # Знак (кольцо стрелок с пламенем) — без плитки: фон баннера и есть фон значка.
    mark_size = 330
    mark = Image.open(BRAND / 'mark.png').convert('RGBA').resize((mark_size, mark_size), Image.LANCZOS)
    mark_x, mark_y = 78, (H - mark_size) // 2
    img.alpha_composite(mark, (mark_x, mark_y))

    # Название — кегль подбирается под ширину правой части. Поля 72 пикселя:
    # Google Play на части экранов обрезает баннер по краям.
    left = mark_x + mark_size + 40
    right = W - 72
    title = 'Ogneliss Share'
    size = 96
    while size > 40:
        f = font(size)
        box = f.getbbox(title)
        if box[2] - box[0] <= right - left:
            break
        size -= 2
    f = font(size)
    box = f.getbbox(title)
    title_w, title_h = box[2] - box[0], box[3] - box[1]

    # Ряд «телефон ⇄ ноутбук ⇄ компьютер».
    h = 64
    parts = [pictogram(PHONE, h, LIGHT), pictogram(SWAP, 34, GOLD), pictogram(LAPTOP, h - 6, LIGHT),
             pictogram(SWAP, 34, GOLD), pictogram(DESKTOP, h - 6, LIGHT)]
    row_w = sum(p.width for p in parts)
    gap = (title_w - row_w) / (len(parts) - 1)
    gap = max(20, min(gap, 46))
    row_w += gap * (len(parts) - 1)

    block_h = title_h + 44 + h
    top = (H - block_h) // 2 - 4
    x0 = left + (right - left - max(title_w, row_w)) / 2
    d = ImageDraw.Draw(img)
    d.text((x0 - box[0], top - box[1]), title, font=f, fill=LIGHT)
    x = x0 + (title_w - row_w) / 2 if row_w < title_w else x0
    y_row = top + title_h + 44
    for p in parts:
        img.alpha_composite(p, (round(x), round(y_row + (h - p.height) / 2)))
        x += p.width + gap
    assert x - gap <= right + 1, 'ряд значков вышел за правое поле'
    return img.convert('RGB')


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    icon = Image.open(ICONS / 'app_icon.png').convert('RGB').resize((512, 512), Image.LANCZOS)
    icon.save(OUT / 'play_icon_512.png', optimize=True)
    feature_graphic().save(OUT / 'play_feature_1024x500.png', optimize=True)
    print('ok')


if __name__ == '__main__':
    main()
