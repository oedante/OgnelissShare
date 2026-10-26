#!/usr/bin/env python3
"""Проверка текстов магазинов в store/: длины полей, языки, адреса.

    python tool/release/check_store.py

Ограничения — как в Play Console и App Store Connect: текст длиннее
магазин просто не сохранит, и узнаётся это в последний момент.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STORE = ROOT / 'store'

PLAY = {'title.txt': 30, 'short_description.txt': 80, 'full_description.txt': 4000}
APPSTORE = {'name.txt': 30, 'subtitle.txt': 30, 'promotional_text.txt': 170, 'description.txt': 4000,
            'keywords.txt': 100, 'support_url.txt': 255, 'marketing_url.txt': 255, 'privacy_url.txt': 255}
TESTFLIGHT = {'description.txt': 4000, 'what_to_test.txt': 4000}
NOTES_LIMIT = 500

for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding='utf-8', errors='replace')
    except (AttributeError, ValueError):
        pass


def text(path):
    return path.read_text(encoding='utf-8').strip()


def check_fields(folder, limits, bad):
    langs = sorted(p for p in folder.iterdir() if p.is_dir())
    if not langs:
        bad.append(f'{folder.relative_to(ROOT)}: нет ни одного языка')
    for lang in langs:
        for name, limit in limits.items():
            f = lang / name
            if not f.exists():
                bad.append(f'{f.relative_to(ROOT)}: нет файла')
                continue
            t = text(f)
            if not t:
                bad.append(f'{f.relative_to(ROOT)}: пусто')
            if len(t) > limit:
                bad.append(f'{f.relative_to(ROOT)}: {len(t)} знаков, можно {limit}')
            if name.endswith('_url.txt') and not re.fullmatch(r'https://\S+', t):
                bad.append(f'{f.relative_to(ROOT)}: не адрес https')
            if name == 'keywords.txt' and (', ' in t or t.endswith(',')):
                bad.append(f'{f.relative_to(ROOT)}: ключевые слова — через запятую без пробелов')
    return [p.name for p in langs]


def main():
    bad = []
    play_langs = check_fields(STORE / 'play', PLAY, bad)
    check_fields(STORE / 'appstore', APPSTORE, bad)
    check_fields(STORE / 'testflight', TESTFLIGHT, bad)
    for f in sorted((STORE / 'whats_new').glob('*.txt')):
        blocks = re.findall(r'<([A-Za-z]{2,3}(?:-[A-Za-z0-9]+)?)>\s*(.*?)\s*</\1>', text(f), re.S)
        langs = sorted(lang for lang, _ in blocks)
        if langs != play_langs:
            bad.append(f'{f.relative_to(ROOT)}: языки {langs}, а у карточек Google Play {play_langs}')
        for lang, t in blocks:
            if len(t) > NOTES_LIMIT:
                bad.append(f'{f.relative_to(ROOT)}: {lang} — {len(t)} знаков, можно {NOTES_LIMIT}')
    for b in bad:
        print('  ✗', b)
    if bad:
        sys.exit(f'Замечаний: {len(bad)}')
    print('Тексты магазинов: всё в порядке.')


if __name__ == '__main__':
    main()
