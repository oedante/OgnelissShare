#!/usr/bin/env python3
"""Значок Ogneliss Share: огненная лиса внутри кольца из двух стрелок обмена.

Оформление — как у остальных приложений линейки (Ogneliss Dialer, Ogneliss
Mail): сланцевый фон #3A4450, светлый знак #D3D8DC, золотое пламя #CCAA6D
со светлой лисой. Пламя с лисой — общий логотип «Огнелисс»
(tool/branding/ogneliss-flame.svg, тот же файл, что на ogneliss.ru).

Запуск (нужны Pillow, numpy, scipy и cairosvg):

    python3 tool/branding/make_icon.py

Что пишется:
  assets/icons/app_icon.png             — 1024, фон во всю площадь (iOS, магазины)
  assets/icons/app_icon_rounded.png     — 1024, скруглённая плитка (Windows, Linux, README)
  assets/icons/app_icon_macos.png       — 1024, плитка с полями по правилам macOS
  assets/icons/app_icon_foreground.png  — передний слой адаптивного значка Android
  assets/icons/app_icon_monochrome.png  — одноцветный слой (тематические значки Android 13+)
  assets/brand/mark.png                 — знак без фона (экран «О приложении»)
  assets/brand/logo.svg                 — векторный логотип (README, сайт)
  docs/images/icon_preview.png          — все варианты рядом
"""
import io
import math
from pathlib import Path

import cairosvg
import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage

ROOT = Path(__file__).resolve().parents[2]
FLAME_SVG = ROOT / 'tool' / 'branding' / 'ogneliss-flame.svg'

SLATE = (0x3A, 0x44, 0x50)
GLYPH = (0xD3, 0xD8, 0xDC)
GOLD = (0xCC, 0xAA, 0x6D)
FOX = (0xF2, 0xF4, 0xF6)

SS = 2048  # рабочий размер, итог уменьшается с сглаживанием


def hexc(c):
    return '#%02X%02X%02X' % c


# ---------------------------------------------------------------- стрелки

def arrows_svg(size, color, ring_r, stroke, cx=None, cy=None):
    """Два дуговых указателя, бегущих друг за другом по кольцу (обмен)."""
    cx = size / 2 if cx is None else cx
    cy = size / 2 if cy is None else cy
    parts = []
    # Дуга A: слева сверху по часовой — к правому краю; дуга B — симметрично снизу.
    for start, end in ((208.0, 322.0), (28.0, 142.0)):
        a0, a1 = math.radians(start), math.radians(end)
        x0, y0 = cx + ring_r * math.cos(a0), cy + ring_r * math.sin(a0)
        x1, y1 = cx + ring_r * math.cos(a1), cy + ring_r * math.sin(a1)
        parts.append(
            f'<path d="M{x0:.1f} {y0:.1f} A{ring_r:.1f} {ring_r:.1f} 0 0 1 {x1:.1f} {y1:.1f}" '
            f'fill="none" stroke="{hexc(color)}" stroke-width="{stroke:.1f}" '
            f'stroke-linecap="round"/>')
        # Наконечник: треугольник по касательной в конце дуги.
        tx, ty = -math.sin(a1), math.cos(a1)      # касательная по часовой
        nx, ny = math.cos(a1), math.sin(a1)       # нормаль наружу
        head_len, head_w = stroke * 1.55, stroke * 1.25
        bx, by = x1 - tx * stroke * 0.25, y1 - ty * stroke * 0.25
        tip = (x1 + tx * head_len, y1 + ty * head_len)
        left = (bx + nx * head_w, by + ny * head_w)
        right = (bx - nx * head_w, by - ny * head_w)
        parts.append(
            f'<path d="M{left[0]:.1f} {left[1]:.1f} L{tip[0]:.1f} {tip[1]:.1f} '
            f'L{right[0]:.1f} {right[1]:.1f} Z" fill="{hexc(color)}" '
            f'stroke="{hexc(color)}" stroke-width="{stroke * 0.32:.1f}" stroke-linejoin="round"/>')
    return '\n'.join(parts)


def render_svg(svg_text, size):
    png = cairosvg.svg2png(bytestring=svg_text.encode('utf-8'),
                           output_width=size, output_height=size)
    return Image.open(io.BytesIO(png)).convert('RGBA')


# ---------------------------------------------------------------- пламя с лисой

def flame_layer(size, box, color=GOLD, fox=FOX):
    """Пламя с лисой в квадрате box=(x, y, side). Лиса — светлая заливка выреза."""
    x, y, side = box
    flame = Image.open(io.BytesIO(cairosvg.svg2png(
        url=str(FLAME_SVG), output_width=side, output_height=side))).convert('RGBA')
    alpha = np.array(flame)[:, :, 3]
    solid = alpha > 127
    # Вырез лисы открыт к краю пламени узкой щелью внизу: закрываем щели
    # «наружным» замыканием и берём самый большой внутренний остаток — лису.
    r = max(2, int(side * 0.037))
    st = ndimage.generate_binary_structure(2, 1)
    grown = ndimage.binary_dilation(solid, structure=st, iterations=r)
    lab, _ = ndimage.label(~grown)
    border = set(np.unique(np.concatenate([lab[0], lab[-1], lab[:, 0], lab[:, -1]]))) - {0}
    outside = ndimage.binary_dilation(np.isin(lab, list(border)), structure=st, iterations=r)
    hole = (~solid) & (~outside)
    lab2, n2 = ndimage.label(hole)
    if n2:
        sizes = ndimage.sum(hole, lab2, range(1, n2 + 1))
        hole = lab2 == (int(np.argmax(sizes)) + 1)
        hole = ndimage.binary_dilation(hole, iterations=2) & ~solid | hole

    layer = np.zeros((side, side, 4), np.uint8)
    if fox is not None:
        layer[hole] = (*fox, 255)
    gold = np.zeros((side, side, 4), np.uint8)
    gold[..., :3] = color
    gold[..., 3] = alpha
    out = Image.alpha_composite(Image.fromarray(layer), Image.fromarray(gold))
    canvas = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    canvas.alpha_composite(out, (x, y))
    return canvas


# ---------------------------------------------------------------- сборка

def glyph(size, scale=1.0, mono=None):
    """Кольцо стрелок + пламя. scale — доля холста, занятая знаком."""
    ring_r = size * 0.335 * scale
    stroke = size * 0.072 * scale
    color = GLYPH if mono is None else mono
    arrows = render_svg(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" '
        f'viewBox="0 0 {size} {size}">{arrows_svg(size, color, ring_r, stroke)}</svg>', size)
    side = int(size * 0.46 * scale)
    fx = (size - side) // 2 + int(side * 0.01)
    fy = (size - side) // 2 - int(side * 0.02)
    if mono is None:
        flame = flame_layer(size, (fx, fy, side))
    else:
        flame = flame_layer(size, (fx, fy, side), color=mono, fox=None)
    return Image.alpha_composite(arrows, flame)


def tile(size, radius_frac=None, margin_frac=0.0, shadow=False):
    img = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    m = int(size * margin_frac)
    d = ImageDraw.Draw(img)
    box = (m, m, size - m - 1, size - m - 1)
    if shadow:
        sh = Image.new('RGBA', (size, size), (0, 0, 0, 0))
        ImageDraw.Draw(sh).rounded_rectangle(
            (m, m + int(size * 0.012), size - m - 1, size - m - 1 + int(size * 0.012)),
            radius=int((size - 2 * m) * (radius_frac or 0)), fill=(0, 0, 0, 90))
        from PIL import ImageFilter
        img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(size * 0.012)))
    if radius_frac:
        d = ImageDraw.Draw(img)
        d.rounded_rectangle(box, radius=int((size - 2 * m) * radius_frac), fill=(*SLATE, 255))
    else:
        d.rectangle(box, fill=(*SLATE, 255))
    return img, m


def compose(size, radius_frac=None, margin_frac=0.0, shadow=False):
    base, m = tile(SS, radius_frac, margin_frac, shadow)
    inner = SS - 2 * m
    g = glyph(inner)
    base.alpha_composite(g, (m, m))
    return base.resize((size, size), Image.LANCZOS)


def logo_svg():
    """Векторный логотип: плитка, стрелки и пламя (лиса — вырез, как на сайте)."""
    size = 1024
    flame_src = FLAME_SVG.read_text(encoding='utf-8')
    inner = flame_src[flame_src.index('>', flame_src.index('<svg')) + 1: flame_src.rindex('</svg>')]
    side = size * 0.46
    fx = (size - side) / 2 + side * 0.01
    fy = (size - side) / 2 - side * 0.02
    s = side / 150.0
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">\n'
        '  <!-- Ogneliss Share: огненная лиса в кольце обмена. Генерируется '
        'tool/branding/make_icon.py -->\n'
        f'  <rect width="1024" height="1024" rx="230" fill="{hexc(SLATE)}"/>\n'
        f'  {arrows_svg(size, GLYPH, size * 0.335, size * 0.072)}\n'
        f'  <g transform="translate({fx:.1f} {fy:.1f}) scale({s:.4f})">{inner}</g>\n'
        '</svg>\n')


def main():
    icons = ROOT / 'assets' / 'icons'
    brand = ROOT / 'assets' / 'brand'
    images = ROOT / 'docs' / 'images'
    for p in (icons, brand, images):
        p.mkdir(parents=True, exist_ok=True)

    full = compose(1024)
    full.convert('RGB').save(icons / 'app_icon.png')
    rounded = compose(1024, radius_frac=0.225)
    rounded.save(icons / 'app_icon_rounded.png')
    # macOS: плитка 824/1024 с полями и мягкой тенью (сетка значков Big Sur и новее).
    mac = compose(1024, radius_frac=0.225, margin_frac=0.0977, shadow=True)
    mac.save(icons / 'app_icon_macos.png')

    # Android: знак в безопасной зоне (66 из 108 dp), фон — цвет в ресурсах.
    fg = Image.new('RGBA', (SS, SS), (0, 0, 0, 0))
    g = glyph(SS, scale=0.66)
    fg.alpha_composite(g)
    fg.resize((1024, 1024), Image.LANCZOS).save(icons / 'app_icon_foreground.png')
    mono = Image.new('RGBA', (SS, SS), (0, 0, 0, 0))
    mono.alpha_composite(glyph(SS, scale=0.66, mono=(255, 255, 255)))
    mono.resize((1024, 1024), Image.LANCZOS).save(icons / 'app_icon_monochrome.png')

    mark = glyph(SS)
    mark.resize((512, 512), Image.LANCZOS).save(brand / 'mark.png')
    (brand / 'logo.svg').write_text(logo_svg(), encoding='utf-8')

    # Превью: плитка, круг, macOS, адаптивный слой на фоне, монохромный.
    prev = Image.new('RGBA', (5 * 260 + 40, 300), (236, 238, 240, 255))
    variants = [rounded, full, mac]
    circle = full.copy()
    msk = Image.new('L', circle.size, 0)
    ImageDraw.Draw(msk).ellipse((0, 0, *circle.size), fill=255)
    circle.putalpha(msk)
    variants.insert(1, circle)
    mono_prev = Image.new('RGBA', (1024, 1024), (40, 44, 52, 255))
    mono_prev.alpha_composite(Image.open(icons / 'app_icon_monochrome.png'))
    variants.append(mono_prev)
    for i, v in enumerate(variants):
        prev.alpha_composite(v.resize((240, 240), Image.LANCZOS), (20 + i * 260, 30))
    prev.convert('RGB').save(images / 'icon_preview.png')
    print('ok')


if __name__ == '__main__':
    main()
