#!/usr/bin/env python3
"""Проверка Android-сборки перед Google Play: страницы памяти 16 КБ.

    python tool/release/check_apk.py build/app/outputs/flutter-apk/*.apk
    python tool/release/check_apk.py dist/ogneliss-share-0.1.0-android.aab

Google Play принимает новые приложения и обновления (целевой Android 15+),
только если их 64-битные библиотеки .so работают на устройствах со
страницами памяти 16 КБ. Для этого:
  · сегменты LOAD каждой 64-битной библиотеки выровнены на 16 КБ и больше;
  · в APK несжатые библиотеки лежат с выравниванием 16 КБ от начала файла
    (в AAB это делает сам Google Play при сборке APK).
32-битные библиотеки (armeabi-v7a) этого не требуют — о них только сведения.

Код выхода 1 — есть нарушения у 64-битных библиотек.
"""
import struct
import sys
import zipfile
from pathlib import Path

PAGE = 16 * 1024
PT_LOAD = 1

for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding='utf-8', errors='replace')
    except (AttributeError, ValueError):
        pass


def load_alignments(elf):
    """Выравнивания сегментов PT_LOAD и разрядность библиотеки."""
    if elf[:4] != b'\x7fELF':
        raise ValueError('не ELF')
    bits = 64 if elf[4] == 2 else 32
    e = '<' if elf[5] == 1 else '>'
    if bits == 64:
        phoff = struct.unpack_from(e + 'Q', elf, 0x20)[0]
        phentsize, phnum = struct.unpack_from(e + 'HH', elf, 0x36)
        align_at, fmt = 0x30, 'Q'
    else:
        phoff = struct.unpack_from(e + 'I', elf, 0x1C)[0]
        phentsize, phnum = struct.unpack_from(e + 'HH', elf, 0x2A)
        align_at, fmt = 0x1C, 'I'
    aligns = []
    for i in range(phnum):
        off = phoff + i * phentsize
        if struct.unpack_from(e + 'I', elf, off)[0] == PT_LOAD:
            aligns.append(struct.unpack_from(e + fmt, elf, off + align_at)[0])
    return bits, aligns


def data_offset(path, info):
    """Где в файле начинаются данные записи zip (по локальному заголовку)."""
    with open(path, 'rb') as f:
        f.seek(info.header_offset)
        header = f.read(30)
    name_len, extra_len = struct.unpack_from('<HH', header, 26)
    return info.header_offset + 30 + name_len + extra_len


def check(path):
    path = Path(path)
    is_apk = path.suffix.lower() == '.apk'
    problems = 0
    found = 0
    with zipfile.ZipFile(path) as z:
        for info in z.infolist():
            if not info.filename.endswith('.so') or '/lib/' not in f'/{info.filename}':
                continue
            found += 1
            name = info.filename
            try:
                bits, aligns = load_alignments(z.read(name))
            except (ValueError, struct.error) as err:
                print(f'  ? {name}: {err}')
                continue
            notes = []
            ok = all(a >= PAGE for a in aligns)
            if not ok:
                notes.append(f'LOAD выровнены на {min(aligns)} байт (нужно {PAGE})')
            if is_apk and info.compress_type == zipfile.ZIP_STORED:
                off = data_offset(path, info)
                if off % PAGE:
                    ok = False
                    notes.append(f'в APK смещение {off} не кратно {PAGE}')
            if bits == 32:
                mark = '·'
                notes = notes and [f'{n} (32 бита — не требуется)' for n in notes]
            elif ok:
                mark = '✓'
            else:
                mark = '✗'
                problems += 1
            print(f'  {mark} {name} ({bits} бит){": " + "; ".join(notes) if notes else ""}')
    if not found:
        print('  библиотек .so нет')
    return problems


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    total = 0
    for p in sys.argv[1:]:
        print(p)
        total += check(p)
    if total:
        print(f'Нарушений у 64-битных библиотек: {total} — Google Play такую сборку не примет.')
        sys.exit(1)
    print('Страницы 16 КБ: всё в порядке.')


if __name__ == '__main__':
    main()
