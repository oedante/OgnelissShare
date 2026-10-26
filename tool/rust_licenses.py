#!/usr/bin/env python3
"""Лицензии библиотек Rust, вошедших в ядро, — для экрана «Лицензии».

Flutter сам показывает лицензии пакетов Dart, а библиотеки Rust ему
неизвестны. Скрипт берёт список из `cargo metadata` (для целей Android,
iOS, macOS, Windows и Linux), читает файлы LICENSE*/COPYING*/NOTICE* каждой
библиотеки и пишет assets/licenses/rust.json: одинаковые тексты
объединены, у каждого — список библиотек.

    python3 tool/rust_licenses.py      # после изменения rust/Cargo.lock
"""
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'assets' / 'licenses' / 'rust.json'
TARGETS = [
    'aarch64-linux-android', 'armv7-linux-androideabi', 'x86_64-linux-android',
    'aarch64-apple-ios', 'aarch64-apple-darwin', 'x86_64-apple-darwin',
    'x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu',
]
PREFIXES = ('license', 'licence', 'copying', 'notice', 'unlicense')
MIT_TEXT = """MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."""


def metadata(target):
    # По одной цели: несколько --filter-platform сразу cargo 1.97 не переваривает.
    cmd = ['cargo', 'metadata', '--format-version', '1', '--locked', '--filter-platform', target]
    out = subprocess.run(cmd, cwd=ROOT / 'rust', check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def normal_deps(meta):
    """Пакеты, достижимые от ядра по обычным зависимостям (без dev и build)."""
    nodes = {n['id']: n for n in meta['resolve']['nodes']}
    root = meta['resolve']['root']
    seen, stack = set(), [root]
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]['deps']:
            if any(k['kind'] in (None, 'normal') for k in dep['dep_kinds']):
                stack.append(dep['pkg'])
    seen.discard(root)
    return seen


def main():
    wanted = set()
    packages = {}
    for target in TARGETS:
        meta = metadata(target)
        wanted |= normal_deps(meta)
        for pkg in meta['packages']:
            packages[pkg['id']] = pkg
    texts = {}
    missing = []
    for pkg in sorted(packages.values(), key=lambda p: (p['name'], p['version'])):
        if pkg['id'] not in wanted:
            continue
        label = f"{pkg['name']} {pkg['version']}"
        folder = Path(pkg['manifest_path']).parent
        files = sorted(p for p in folder.iterdir()
                       if p.is_file() and p.name.lower().startswith(PREFIXES))
        if not files:
            authors = ', '.join(pkg.get('authors') or []) or 'авторы пакета'
            missing.append(f"{label} — {pkg.get('license') or '?'} — {authors}")
            continue
        for f in files:
            text = f.read_text(encoding='utf-8', errors='replace').strip()
            texts.setdefault(text, set()).add(pkg['name'])
    entries = [{'packages': sorted(names), 'text': text} for text, names in texts.items()]
    entries.sort(key=lambda e: e['packages'])
    if missing:
        entries.append({
            'packages': ['Rust: лицензия указана в Cargo.toml'],
            'text': 'В этих пакетах нет файла лицензии; лицензия и авторы — из их Cargo.toml. '
                    'Тексты лицензий Apache-2.0 и MIT — ниже и в файле LICENSE приложения.\n\n'
                    + '\n'.join(sorted(missing)) + '\n\n' + MIT_TEXT,
        })
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(entries, ensure_ascii=False, indent=0), encoding='utf-8')
    print(f'{len(wanted)} пакетов, {len(entries)} текстов, {OUT.stat().st_size // 1024} КБ, без файлов: {len(missing)}')


if __name__ == '__main__':
    main()
