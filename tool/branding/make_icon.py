#!/usr/bin/env python3
"""Значок Ogneliss Share: огненная лиса внутри кольца из двух стрелок обмена.

Оформление — как у остальных приложений линейки (Ogneliss Dialer, Ogneliss
Mail): сланцевый фон #3A4450, светлый знак, золотое пламя #CCAA6D со светлой
лисой. Пламя с лисой — общий знак «Огнелисс» (tool/branding/ogneliss-mark.svg,
тот же, что у Ogneliss Mail и Dialer): лиса с двумя ушами — отверстия в пламени.

Стрелки — в духе пламени: сужаются к хвосту, как языки огня, и кончаются
острым наконечником с вогнутыми сторонами; объём дают переход от белого к
светло-серому и мягкая тень. Одноцветный слой (Android 13+) — те же формы
без перехода и тени.

Запуск (нужны Pillow, numpy, scipy, cairosvg и opencv-python):

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
import cv2
import numpy as np
from PIL import Image, ImageDraw, ImageFilter
from scipy import ndimage

ROOT = Path(__file__).resolve().parents[2]
FLAME_SVG = ROOT / 'tool' / 'branding' / 'ogneliss-mark.svg'
MARK_UNITS = 5016  # viewBox знака

SLATE = (0x3A, 0x44, 0x50)
GLYPH = (0xD3, 0xD8, 0xDC)
GOLD = (0xCC, 0xAA, 0x6D)
FOX = (0xF2, 0xF4, 0xF6)

SS = 2048  # рабочий размер, итог уменьшается с сглаживанием


def hexc(c):
    return '#%02X%02X%02X' % c


# ---------------------------------------------------------------- стрелки

ARROW_TOP = '#FFFFFF'
ARROW_BOTTOM = '#C5CCD3'
# Две дуги, бегущие друг за другом по кольцу (градусы, по часовой от «3 часов»).
ARCS = ((200.0, 306.0), (20.0, 126.0))


def _pt(cx, cy, r, deg):
    a = math.radians(deg)
    return cx + r * math.cos(a), cy + r * math.sin(a)


def swoosh_d(cx, cy, ring_r, a0, a1, stroke, n=140):
    """Контур одной стрелки (путь SVG): хвост — острый, тело плавно
    расширяется, наконечник — по касательной к кольцу, острие повторяет его
    изгиб, стороны слегка вогнуты."""
    w_tail, w_neck = stroke * 0.03, stroke * 0.80
    outer, inner = [], []
    for i in range(n + 1):
        t = i / n
        a = a0 + (a1 - a0) * t
        w = w_tail + (w_neck - w_tail) * (1 - (1 - t) ** 2.3)
        outer.append(_pt(cx, cy, ring_r + w, a))
        inner.append(_pt(cx, cy, ring_r - w, a))
    ar = math.radians(a1)
    nx, ny = math.cos(ar), math.sin(ar)       # наружу от центра
    tx, ty = -math.sin(ar), math.cos(ar)      # вперёд, по часовой
    ncx, ncy = cx + ring_r * nx, cy + ring_r * ny
    length, half, back = stroke * 2.05, stroke * 1.38, stroke * 0.30
    bend = length * length / (2 * ring_r)
    tip = (ncx + tx * length - nx * bend, ncy + ty * length - ny * bend)
    barb_out = (ncx + nx * half - tx * back, ncy + ny * half - ty * back)
    barb_in = (ncx - nx * half - tx * back, ncy - ny * half - ty * back)
    axis = ((ncx + tip[0]) / 2, (ncy + tip[1]) / 2)

    def ctrl(p, q, k=0.16):
        mx, my = (p[0] + q[0]) / 2, (p[1] + q[1]) / 2
        return mx + (axis[0] - mx) * k, my + (axis[1] - my) * k

    f = lambda p: f'{p[0]:.1f} {p[1]:.1f}'  # noqa: E731
    d = 'M' + f(outer[0]) + ''.join(' L' + f(p) for p in outer[1:])
    d += f' L{f(barb_out)} Q{f(ctrl(barb_out, tip))} {f(tip)} Q{f(ctrl(tip, barb_in))} {f(barb_in)}'
    d += ''.join(' L' + f(p) for p in reversed(inner)) + ' Z'
    return d


def arrows_svg(size, ring_r, stroke, fill, cx=None, cy=None):
    """Обе стрелки; fill — цвет или url(#…) перехода."""
    cx = size / 2 if cx is None else cx
    cy = size / 2 if cy is None else cy
    return '\n'.join(f'<path d="{swoosh_d(cx, cy, ring_r, a0, a1, stroke)}" fill="{fill}"/>'
                     for a0, a1 in ARCS)


def arrows_gradient(gid='arrows'):
    """Свет сверху: у каждой стрелки — от белого к светло-серому."""
    return (f'<linearGradient id="{gid}" x1="0" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="{ARROW_TOP}"/><stop offset="1" stop-color="{ARROW_BOTTOM}"/>'
            f'</linearGradient>')


def with_shadow(layer, size):
    """Мягкая тень под знаком — он чуть приподнят над фоном."""
    alpha = layer.split()[3]
    shadow = Image.new('RGBA', layer.size, (8, 12, 18, 0))
    shadow.putalpha(alpha.point(lambda v: int(v * 0.6)).filter(ImageFilter.GaussianBlur(size * 0.009)))
    out = Image.new('RGBA', layer.size, (0, 0, 0, 0))
    out.alpha_composite(shadow, (0, int(size * 0.011)))
    out.alpha_composite(layer)
    return out


def render_svg(svg_text, size):
    png = cairosvg.svg2png(bytestring=svg_text.encode('utf-8'),
                           output_width=size, output_height=size)
    return Image.open(io.BytesIO(png)).convert('RGBA')


# ---------------------------------------------------------------- пламя с лисой

def fox_mask(side):
    """Знак в квадрате side: (альфа пламени, маска лисы). Лиса — отверстия в
    пламени: туловище с первым ухом открыто к краю узкой щелью внизу (её
    закрываем «наружным» замыканием и берём самое большое отверстие), второе
    ухо — отдельное, полностью замкнутое отверстие. Вырезы между языками
    пламени тоже узкие, но они не замкнуты и в лису не попадают."""
    flame = Image.open(io.BytesIO(cairosvg.svg2png(
        url=str(FLAME_SVG), output_width=side, output_height=side))).convert('RGBA')
    alpha = np.array(flame)[:, :, 3]
    solid = alpha > 127

    def edge_labels(lab):
        return set(np.unique(np.concatenate([lab[0], lab[-1], lab[:, 0], lab[:, -1]]))) - {0}

    # Полностью замкнутые отверстия (второе ухо).
    lab0, _ = ndimage.label(~solid)
    enclosed = (~solid) & ~np.isin(lab0, list(edge_labels(lab0)))
    # Туловище: самое большое отверстие после закрытия щели.
    r = max(2, int(side * 0.037))
    st = ndimage.generate_binary_structure(2, 1)
    grown = ndimage.binary_dilation(solid, structure=st, iterations=r)
    lab, _ = ndimage.label(~grown)
    outside = ndimage.binary_dilation(np.isin(lab, list(edge_labels(lab))), structure=st, iterations=r)
    hole = (~solid) & (~outside)
    lab2, n2 = ndimage.label(hole)
    if n2:
        sizes = ndimage.sum(hole, lab2, range(1, n2 + 1))
        hole = lab2 == (int(np.argmax(sizes)) + 1)
        hole = ndimage.binary_dilation(hole, iterations=2) & ~solid | hole
    return alpha, hole | enclosed


def flame_layer(size, box, color=GOLD, fox=FOX):
    """Пламя с лисой в квадрате box=(x, y, side). Лиса — светлая заливка отверстий."""
    x, y, side = box
    alpha, hole = fox_mask(side)
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


def fox_path_d(px=2048):
    """Контур лисы в единицах знака — для векторного логотипа (там она
    светлая, как на значке, а не вырез)."""
    _, hole = fox_mask(px)
    contours, _ = cv2.findContours(hole.astype(np.uint8), cv2.RETR_CCOMP, cv2.CHAIN_APPROX_NONE)
    k = MARK_UNITS / px
    parts = []
    for c in contours:
        if cv2.contourArea(c) < px * 0.5:
            continue
        c = cv2.approxPolyDP(c, 0.6, True)
        pts = ' L'.join(f'{p[0][0] * k:.0f} {p[0][1] * k:.0f}' for p in c)
        parts.append(f'M{pts} Z')
    return ' '.join(parts)


def mark_path_d():
    """Путь пламени из файла знака."""
    import re
    src = FLAME_SVG.read_text(encoding='utf-8')
    return re.search(r'<path[^>]*\sd="([^"]*)"', src, re.S).group(1)


# ---------------------------------------------------------------- сборка

def glyph(size, scale=1.0, mono=None):
    """Кольцо стрелок + пламя. scale — доля холста, занятая знаком."""
    ring_r = size * 0.325 * scale
    stroke = size * 0.074 * scale
    head = f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}">'
    if mono is None:
        arrows = render_svg(f'{head}<defs>{arrows_gradient()}</defs>'
                            f'{arrows_svg(size, ring_r, stroke, "url(#arrows)")}</svg>', size)
        arrows = with_shadow(arrows, size)
    else:
        arrows = render_svg(f'{head}{arrows_svg(size, ring_r, stroke, hexc(mono))}</svg>', size)
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
        img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(size * 0.012)))
    shape = Image.new('L', (size, size), 0)
    if radius_frac:
        ImageDraw.Draw(shape).rounded_rectangle(box, radius=int((size - 2 * m) * radius_frac), fill=255)
    else:
        ImageDraw.Draw(shape).rectangle(box, fill=255)
    plate = Image.new('RGBA', (size, size), (*SLATE, 255))
    # Едва заметный свет сверху — фон не плоский.
    glow = Image.new('L', (size, size), 0)
    ImageDraw.Draw(glow).ellipse((-size * 0.2, -size * 0.55, size * 1.2, size * 0.75), fill=26)
    light = Image.new('RGBA', (size, size), (255, 255, 255, 0))
    light.putalpha(glow.filter(ImageFilter.GaussianBlur(size * 0.12)))
    plate.alpha_composite(light)
    plate.putalpha(shape)
    img.alpha_composite(plate)
    return img, m


def compose(size, radius_frac=None, margin_frac=0.0, shadow=False):
    base, m = tile(SS, radius_frac, margin_frac, shadow)
    inner = SS - 2 * m
    g = glyph(inner)
    base.alpha_composite(g, (m, m))
    return base.resize((size, size), Image.LANCZOS)


def logo_svg():
    """Векторный логотип: плитка, стрелки, пламя и светлая лиса — как значок."""
    size = 1024
    side = size * 0.46
    fx = (size - side) / 2 + side * 0.01
    fy = (size - side) / 2 - side * 0.02
    s = side / MARK_UNITS
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">\n'
        '  <!-- Ogneliss Share: огненная лиса в кольце обмена. Генерируется '
        'tool/branding/make_icon.py -->\n'
        f'  <defs>{arrows_gradient()}'
        '<linearGradient id="plate" x1="0" y1="0" x2="0" y2="1">'
        '<stop offset="0" stop-color="#46505C"/><stop offset="1" stop-color="#3A4450"/>'
        '</linearGradient></defs>\n'
        '  <rect width="1024" height="1024" rx="230" fill="url(#plate)"/>\n'
        f'  {arrows_svg(size, size * 0.325, size * 0.074, "url(#arrows)")}\n'
        f'  <g transform="translate({fx:.1f} {fy:.1f}) scale({s:.6f})">\n'
        f'    <path fill="{hexc(FOX)}" fill-rule="evenodd" d="{fox_path_d()}"/>\n'
        f'    <path fill="{hexc(GOLD)}" fill-rule="evenodd" d="{mark_path_d()}"/>\n'
        '  </g>\n'
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
