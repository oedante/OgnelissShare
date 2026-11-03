// Проверка страницы получателя /share/get/ (docs/WEB_P2P.md) в настоящем Chromium:
//   node site/test/get_page_test.mjs
// Нужны Node 20+ и playwright (require('playwright')). Что происходит:
//  1. собирается сайт (site/build_share_site.py), site/out раздаёт маленький сервер на localhost,
//     он же — поддельная служба связи (раздел 2) в памяти;
//  2. «отправитель» — вторая страница Chromium: ждёт предложения, расшифровывает их тем же ключом,
//     отвечает по WebRTC и отдаёт два файла по протоколу канала (раздел 5);
//  3. страница получателя качает файлы, а тест сверяет сохранённые байты с исходными.
// Снимки экрана — в каталог $SHOT_DIR (по умолчанию /tmp/ogneliss-get-shots).
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const { chromium } = createRequire(import.meta.url)('playwright');

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SITE = path.resolve(HERE, '..');
const OUT = path.join(SITE, 'out');
const SHOTS = process.env.SHOT_DIR || path.join(os.tmpdir(), 'ogneliss-get-shots');
fs.mkdirSync(SHOTS, { recursive: true });

// ── данные «отправителя»: одинаково считаются здесь и на странице отправителя ──

const gen = (n) => {
  const u = Buffer.alloc(n);
  for (let i = 0; i < n; i++) u[i] = (i * 7 + (i >> 8)) & 255;
  return u;
};
const FILES = [
  { i: 0, name: 'Отпуск/photo.bin', saved: 'photo.bin', size: 300 * 1024 },
  { i: 1, name: 'docs/<img src=x onerror=alert(1)>.bin', saved: '_img src=x onerror=alert(1)_.bin', size: 3 * 1024 * 1024 },
];
for (const f of FILES) f.data = gen(f.size);
const DEVICE = 'Pixel <b>7</b>';

const b64u = (b) => Buffer.from(b).toString('base64url');
const newLink = () => ({ room: b64u(crypto.randomBytes(16)), key: b64u(crypto.randomBytes(32)), owner: b64u(crypto.randomBytes(16)) });

// ── сервер: статика как в nginx (site/nginx-share.conf) и поддельная служба связи ──

const PAGES = { '/share/get/': 'share/get.html', '/share/ru/get/': 'ru/share/get.html' };
const TYPES = { '.html': 'text/html; charset=utf-8', '.png': 'image/png', '.svg': 'image/svg+xml' };
const rooms = new Map();

const reply = (res, status, obj) => {
  const body = obj === undefined ? '' : JSON.stringify(obj);
  res.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
  res.end(body);
};

// Долгий опрос: ready() возвращает null, пока ответа нет; poke(room) будит ожидающих, когда в комнате что-то изменилось.
function longPoll(room, ready, wait, send) {
  const v = ready();
  if (v !== null || wait <= 0) return send(v);
  const w = {};
  const done = (x) => { clearTimeout(w.timer); room.waiters.delete(w); send(x); };
  w.poke = () => { const x = ready(); if (x !== null) done(x); };
  w.timer = setTimeout(() => done(ready()), wait * 1000);
  room.waiters.add(w);
}
const poke = (room) => [...room.waiters].forEach((w) => w.poke());

function handleApi(req, res, url, body) {
  const parts = url.pathname.replace(/^\/share\/api\/v1\/?/, '').split('/').filter(Boolean);
  const j = () => { try { return JSON.parse(body); } catch { return {}; } };
  const q = (k, d = 0) => Number(url.searchParams.get(k) ?? d);
  if (parts[0] === 'health') return res.end('ok');
  if (parts[0] !== 'rooms' || !parts[1]) return reply(res, 404);
  const name = parts[1];
  const room = rooms.get(name);
  const owner = req.headers['x-owner'];
  const sub = parts[2];
  if (!sub && req.method === 'PUT') {
    if (room && room.owner !== owner) return reply(res, 409);
    const ttl = j().ttl;
    rooms.set(name, room ?? { owner, offers: [], answers: new Map(), waiters: new Set(), seq: 0 });
    return reply(res, 200, { expires: Math.floor(Date.now() / 1000) + ttl });
  }
  if (!room) return reply(res, 404);
  if (!sub && req.method === 'DELETE') return room.owner === owner ? (rooms.delete(name), reply(res, 204)) : reply(res, 403);
  if (sub === 'offers' && req.method === 'POST') {
    const sid = b64u(crypto.randomBytes(9));
    room.offers.push({ seq: ++room.seq, sid, msg: j().msg });
    if (room.autoAnswer) room.answers.set(sid, room.autoAnswer); // для проверки неверного ответа
    poke(room);
    return reply(res, 201, { sid });
  }
  if (sub === 'offers' && req.method === 'GET') {
    if (room.owner !== owner) return reply(res, 403);
    const after = q('after');
    const list = () => room.offers.filter((o) => o.seq > after);
    return longPoll(room, () => (list().length ? list() : null), q('wait'), (v) => reply(res, 200, { offers: v ?? [] }));
  }
  if (sub === 'answers' && parts[3] && req.method === 'PUT') {
    if (room.owner !== owner) return reply(res, 403);
    room.answers.set(parts[3], j().msg);
    poke(room);
    return reply(res, 204);
  }
  if (sub === 'answers' && parts[3] && req.method === 'GET') {
    const sid = parts[3];
    if (!room.offers.some((o) => o.sid === sid)) return reply(res, 404);
    room.polls = (room.polls ?? 0) + 1;
    return longPoll(room, () => room.answers.get(sid) ?? null, Math.min(q('wait'), room.capWait ?? Infinity),
      (v) => (v === null ? reply(res, 204) : reply(res, 200, { msg: v })));
  }
  return reply(res, 404);
}

function serve() {
  return http.createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const body = Buffer.concat(chunks).toString('utf8');
      if (url.pathname.startsWith('/share/api/v1')) return handleApi(req, res, url, body);
      if (url.pathname === '/__sender') {
        res.writeHead(200, { 'Content-Type': 'text/html' });
        return res.end('<!doctype html><title>sender</title>');
      }
      // /dialer/logo.svg (логотип в шапке) лежит на главном сайте — берём тот же значок из репозитория.
      const full = url.pathname === '/dialer/logo.svg' ? path.join(SITE, '..', 'assets', 'brand', 'logo.svg')
        : path.join(OUT, PAGES[url.pathname] ?? url.pathname.replace(/^\//, ''));
      if (!(full.startsWith(OUT) || url.pathname === '/dialer/logo.svg') || !fs.existsSync(full) || fs.statSync(full).isDirectory()) {
        res.writeHead(404);
        return res.end('not found');
      }
      res.writeHead(200, { 'Content-Type': TYPES[path.extname(full)] ?? 'application/octet-stream' });
      res.end(fs.readFileSync(full));
    });
  });
}

// ── «отправитель»: страница Chromium, играющая роль приложения ──

async function startSender(page, link, opts = {}) {
  await page.evaluate(async ({ room, key, owner, files, device, slow, helloSizes, answerDelay, noHello }) => {
    const enc = new TextEncoder();
    const dec = new TextDecoder();
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const de = (s) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/')), (c) => c.charCodeAt(0));
    const en = (u) => btoa(String.fromCharCode(...u)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
    const k = await crypto.subtle.importKey('raw', de(key), 'AES-GCM', false, ['encrypt', 'decrypt']);
    const gen = (n) => {
      const u = new Uint8Array(n);
      for (let i = 0; i < n; i++) u[i] = (i * 7 + (i >> 8)) & 255;
      return u;
    };
    const data = files.map((f) => gen(f.size));
    const hdr = { 'X-Owner': owner, 'Content-Type': 'application/json' };
    const base = '/share/api/v1/rooms/' + room;
    const s = (window.__s = { stop: false, slow, lie: {}, err: {}, log: [], ready: 0, chans: [], pcs: [], langs: [], gets: [], cancels: 0, errors: [] });

    const handle = async (o) => {
      const b = de(o.msg);
      const plain = JSON.parse(dec.decode(await crypto.subtle.decrypt(
        { name: 'AES-GCM', iv: b.subarray(0, 12), additionalData: enc.encode('ogshare-v1|offer|' + room) }, k, b.subarray(12))));
      s.langs.push(plain.lang);
      const pc = new RTCPeerConnection({ iceServers: [] });
      s.pcs.push(pc);
      const tok = { n: 0, cancelled: false };
      const serveFile = async (ch, m) => {
        const my = ++tok.n;
        tok.cancelled = false;
        s.gets.push(m.i);
        const d = data[m.i];
        if (s.err[m.i]) return ch.send(JSON.stringify({ t: 'error', i: m.i, code: s.err[m.i] }));
        ch.send(JSON.stringify({ t: 'file', i: m.i, size: s.lie[m.i] ?? d.length, offset: m.offset || 0 }));
        for (let off = m.offset || 0; off < d.length; off += 16384) {
          if (tok.n !== my || tok.cancelled) return;
          while (ch.bufferedAmount > (1 << 20)) await sleep(2);
          ch.send(d.subarray(off, Math.min(off + 16384, d.length)));
          if (s.slow) await sleep(s.slow);
        }
        ch.send(JSON.stringify({ t: 'end', i: m.i }));
      };
      pc.ondatachannel = (ev) => {
        const ch = ev.channel;
        ch.binaryType = 'arraybuffer';
        s.chans.push(ch);
        s.log.push('datachannel ' + ch.readyState + '@' + Math.round(performance.now()));
        ch.addEventListener('open', () => s.log.push('open@' + Math.round(performance.now())));
        ch.onmessage = (e) => {
          const m = JSON.parse(e.data);
          if (m.t === 'get') serveFile(ch, m).catch((err) => s.errors.push(String(err)));
          if (m.t === 'ready') {
            s.ready++;
            hello();
          }
          if (m.t === 'cancel') {
            tok.cancelled = true;
            s.cancels++;
          }
        };
        const hello = () => s.log.push('hello@' + Math.round(performance.now())) && ch.send(JSON.stringify({
          t: 'hello', v: 1, name: device, expires: Math.floor(Date.now() / 1000) + 3600,
          files: files.map((f, i) => ({ i, name: f.name, size: helloSizes?.[i] ?? f.size })),
        }));
        if (noHello) return; // как будто первое hello потерялось в пути
        if (ch.readyState === 'open') hello(); else ch.onopen = hello;
      };
      await pc.setRemoteDescription({ type: 'offer', sdp: plain.sdp });
      await pc.setLocalDescription(await pc.createAnswer());
      if (pc.iceGatheringState !== 'complete') {
        await new Promise((r) => {
          pc.onicegatheringstatechange = () => pc.iceGatheringState === 'complete' && r();
          setTimeout(r, 4000);
        });
      }
      const nonce = crypto.getRandomValues(new Uint8Array(12));
      const ct = new Uint8Array(await crypto.subtle.encrypt(
        { name: 'AES-GCM', iv: nonce, additionalData: enc.encode('ogshare-v1|answer|' + room + '|' + o.sid) }, k,
        enc.encode(JSON.stringify({ sdp: pc.localDescription.sdp }))));
      const msg = en(new Uint8Array([...nonce, ...ct]));
      await sleep(answerDelay);
      await fetch(base + '/answers/' + o.sid, { method: 'PUT', headers: hdr, body: JSON.stringify({ msg }) });
    };

    await fetch(base, { method: 'PUT', headers: hdr, body: JSON.stringify({ ttl: 600 }) });
    (async () => {
      let after = 0;
      while (!s.stop) {
        const r = await fetch(base + '/offers?after=' + after + '&wait=2', { headers: hdr });
        for (const o of (await r.json()).offers) {
          after = Math.max(after, o.seq);
          handle(o).catch((err) => s.errors.push(String(err)));
        }
      }
    })();
  }, { ...link, files: FILES.map(({ name, size }) => ({ name, size })), device: DEVICE, slow: opts.slow ?? 0, helloSizes: opts.helloSizes, answerDelay: opts.answerDelay ?? 0, noHello: opts.noHello });
}

// ── проверки ──

async function step(name, fn) {
  const t = Date.now();
  await fn();
  console.log(`  ok  ${name} (${((Date.now() - t) / 1000).toFixed(1)} с)`);
}

const text = (page, sel) => page.locator(sel).innerText();

async function receiverPage(ctx, base, link, { lang = 'en', hash } = {}) {
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  page.errors = errors;
  await page.addInitScript(() => {
    const orig = RTCPeerConnection.prototype.createDataChannel;
    RTCPeerConnection.prototype.createDataChannel = function (...a) {
      const ch = orig.apply(this, a);
      const log = (t) => (window.__rx = window.__rx || []).push(t + '@' + Math.round(performance.now()));
      ch.addEventListener('open', () => log('open'));
      ch.addEventListener('message', (e) => log('msg:' + (typeof e.data === 'string' ? e.data.slice(0, 30) : 'bin')));
      ch.addEventListener('close', () => log('close'));
      return ch;
    };
  });
  const p = lang === 'ru' ? '/share/ru/get/' : '/share/get/';
  await page.goto(`${base}${p}?api=${encodeURIComponent(base + '/share/api/v1')}#${hash ?? link.room + '.' + link.key}`);
  return page;
}

const sha = (buf) => crypto.createHash('sha256').update(buf).digest('hex');

async function main() {
  console.log('Сборка сайта…');
  execFileSync('python3', [path.join(SITE, 'build_share_site.py')], { stdio: 'inherit' });

  const server = serve();
  await new Promise((r) => server.listen(0, r));
  const base = `http://localhost:${server.address().port}`;
  const browser = await chromium.launch();
  const failures = [];
  try {
    // Отправитель: одна страница на все проверки, комнат у неё несколько.
    const senderCtx = await browser.newContext();
    const sender = await senderCtx.newPage();
    await sender.goto(base + '/__sender');
    const link = newLink();
    await startSender(sender, link);

    // 1. Blob-путь (в Chromium showSaveFilePicker удаляем), английская страница.
    const blobCtx = await browser.newContext({ viewport: { width: 390, height: 800 }, deviceScaleFactor: 2, acceptDownloads: true });
    await blobCtx.addInitScript(() => { delete window.showSaveFilePicker; delete window.showDirectoryPicker; });
    let page;
    await step('список файлов (EN): имя устройства, размеры, кнопки', async () => {
      page = await receiverPage(blobCtx, base, link);
      await page.waitForSelector('#l-files li', { timeout: 40000 });
      assert.equal(await page.evaluate(() => typeof window.showSaveFilePicker), 'undefined');
      assert.equal(await text(page, '#l-from'), `Files from “${DEVICE}”`);
      assert.equal(await page.locator('#l-from b').count(), 0, 'имя устройства попало в разметку');
      assert.match(await text(page, '#l-valid'), /^Link valid for (59|60) more min$/);
      const items = await page.locator('#l-files li').allInnerTexts();
      assert.equal(items.length, 2);
      assert.match(items[0], /Отпуск\/photo\.bin\s+300\.0 KB\s+Download/);
      assert.match(items[1], /docs\/<img src=x onerror=alert\(1\)>\.bin\s+3\.0 MB\s+Download/);
      assert.equal(await page.locator('#l-files img').count(), 0, 'имя файла попало в разметку');
      assert.equal(await text(page, '#l-all'), 'Download all (2 files, 3.3 MB)');
      assert.deepEqual(await sender.evaluate(() => window.__s.langs), ['en']);
      // Скриншоты списка: светлая и тёмная темы.
      await page.emulateMedia({ colorScheme: 'light' });
      await page.screenshot({ path: path.join(SHOTS, 'list-light.png'), fullPage: true });
      await page.emulateMedia({ colorScheme: 'dark' });
      await page.screenshot({ path: path.join(SHOTS, 'list-dark.png'), fullPage: true });
      await page.emulateMedia({ colorScheme: 'light' });
    });

    const saveOf = async (dl) => fs.readFileSync(await dl.path());
    await step('скачать первый файл (Blob): байты совпадают', async () => {
      const [dl] = await Promise.all([page.waitForEvent('download'), page.locator('#l-files li').nth(0).locator('button').click()]);
      assert.equal(dl.suggestedFilename(), FILES[0].saved);
      assert.ok((await saveOf(dl)).equals(FILES[0].data));
      await page.waitForFunction(() => document.getElementById('l-note-text').textContent.startsWith('Done'));
      assert.equal(await text(page, '#l-note-text'), 'Done — saved: photo.bin');
    });

    await step('«Скачать всё» (Blob): оба файла по очереди', async () => {
      const got = [];
      page.on('download', (d) => got.push(d));
      await page.locator('#l-all').click();
      await page.waitForFunction(() => document.getElementById('l-note-text').textContent.startsWith('Done'), null, { timeout: 30000 });
      await new Promise((r) => setTimeout(r, 300));
      assert.equal(got.length, 2);
      assert.equal(got[0].suggestedFilename(), FILES[0].saved);
      assert.equal(got[1].suggestedFilename(), FILES[1].saved);
      assert.ok((await saveOf(got[0])).equals(FILES[0].data));
      assert.ok((await saveOf(got[1])).equals(FILES[1].data));
      assert.equal(await text(page, '#l-note-text'), 'Done — saved 2 files');
      assert.deepEqual(await sender.evaluate(() => window.__s.gets.slice(-2)), [0, 1], 'файлы просятся по одному и по очереди');
    });
    assert.deepEqual(page.errors, []);

    // 2. Отмена посреди файла и повторная загрузка того же файла.
    await step('отмена: страница шлёт cancel, затем файл скачивается целиком', async () => {
      await sender.evaluate(() => { window.__s.slow = 15; });
      await page.locator('#l-files li').nth(1).locator('button').click();
      await page.waitForFunction(() => / of 3\.0 MB/.test(document.getElementById('p-text').textContent), null, { timeout: 15000 });
      await page.screenshot({ path: path.join(SHOTS, 'progress-light.png'), fullPage: true });
      const before = await sender.evaluate(() => window.__s.cancels);
      await page.locator('#p-cancel').click();
      await page.waitForFunction(() => document.getElementById('l-note-text').textContent === 'Download cancelled.');
      await sender.waitForFunction((n) => window.__s.cancels === n + 1, before);
      assert.equal(await page.locator('#l-prog').isHidden(), true);
      await sender.evaluate(() => { window.__s.slow = 0; });
      const [dl] = await Promise.all([page.waitForEvent('download'), page.locator('#l-files li').nth(1).locator('button').click()]);
      assert.ok((await saveOf(dl)).equals(FILES[1].data));
    });

    await step('ошибки файла: размер не сошёлся, файл не найден', async () => {
      const row = page.locator('#l-files li').nth(1).locator('button');
      await sender.evaluate(() => { window.__s.lie[1] = 3 * 1024 * 1024 + 10; });
      await row.click();
      await page.waitForFunction(() => document.getElementById('l-note').dataset.k === 'err');
      assert.equal(await text(page, '#l-note-text'), 'Could not download “docs/<img src=x onerror=alert(1)>.bin”. The file arrived incomplete. Please try again.');
      await page.screenshot({ path: path.join(SHOTS, 'file-error-light.png'), fullPage: true });
      await sender.evaluate(() => { window.__s.lie = {}; window.__s.err[1] = 'not_found'; });
      await row.click();
      await page.waitForFunction(() => document.getElementById('l-note-text').textContent.includes('not found'));
      await sender.evaluate(() => { window.__s.err = {}; });
      assert.equal(await row.isEnabled(), true, 'после ошибки кнопки снова доступны');
    });

    // 3. Обрыв связи и повтор.
    await step('обрыв канала: ошибка «Connection lost», «Try again» подключает заново', async () => {
      await sender.evaluate(() => window.__s.chans.forEach((c) => c.close()));
      await page.waitForSelector('#v-error:not([hidden])', { timeout: 15000 });
      assert.equal(await text(page, '#e-title'), 'Connection lost');
      assert.equal(await page.locator('#e-retry').isVisible(), true);
      await page.screenshot({ path: path.join(SHOTS, 'error-lost-light.png'), fullPage: true });
      await page.locator('#e-retry').click();
      await page.waitForSelector('#l-files li', { timeout: 40000 });
      assert.equal(await page.locator('#v-error').isHidden(), true);
      assert.equal((await sender.evaluate(() => window.__s.langs)).length, 2, 'повтор — новое предложение');
    });
    await page.close();
    await blobCtx.close();

    // 4. Русская страница: строки, единицы, запятая в дробях, склонение.
    await step('русская страница: «Скачать», КБ/МБ с запятой, «2 файла»', async () => {
      const ctx = await browser.newContext({ viewport: { width: 360, height: 800 } });
      await ctx.addInitScript(() => { delete window.showSaveFilePicker; delete window.showDirectoryPicker; });
      const ru = await receiverPage(ctx, base, link, { lang: 'ru' });
      await ru.waitForSelector('#l-files li', { timeout: 40000 });
      assert.equal(await text(ru, '#l-from'), `Файлы с устройства «${DEVICE}»`);
      assert.match(await text(ru, '#l-valid'), /^Ссылка действует ещё (59|60) мин$/);
      const items = await ru.locator('#l-files li').allInnerTexts();
      assert.match(items[0], /300,0 КБ\s+Скачать/);
      assert.match(items[1], /3,0 МБ\s+Скачать/);
      assert.equal(await text(ru, '#l-all'), 'Скачать всё (2 файла, 3,3 МБ)');
      assert.deepEqual((await sender.evaluate(() => window.__s.langs)).slice(-1), ['ru']);
      // Переключатель языка не теряет «#…».
      assert.equal(await ru.locator('.langs a[hreflang="en"]').evaluate((a) => a.pathname + a.hash), `/share/get/#${link.room}.${link.key}`);
      await ru.screenshot({ path: path.join(SHOTS, 'list-ru-360.png'), fullPage: true });
      assert.deepEqual(ru.errors, []);
      await ctx.close();
    });

    // 5. Запись потоком в файл (showSaveFilePicker) и в папку (showDirectoryPicker) — на подставных объектах.
    await step('showSaveFilePicker / showDirectoryPicker: запись потоком, имена в папке не затираются', async () => {
      const ctx = await browser.newContext({ acceptDownloads: true });
      await ctx.addInitScript(() => {
        window.__saved = {};
        window.__picked = [];
        window.__dir = new Set(['photo.bin']); // в «папке» такой файл уже есть
        const writable = (name) => {
          const parts = [];
          return {
            write: async (b) => { await new Promise((r) => setTimeout(r, 0)); parts.push(new Uint8Array(b)); },
            close: async () => {
              const all = new Uint8Array(parts.reduce((a, p) => a + p.length, 0));
              let o = 0;
              for (const p of parts) { all.set(p, o); o += p.length; }
              window.__saved[name] = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', all)), (x) => x.toString(16).padStart(2, '0')).join('') + ':' + all.length;
            },
            abort: async () => { window.__aborted = (window.__aborted || 0) + 1; },
          };
        };
        window.showSaveFilePicker = async (o) => { window.__picked.push(o.suggestedName); return { createWritable: async () => writable('save:' + o.suggestedName) }; };
        window.showDirectoryPicker = async () => ({
          getFileHandle: async (name, o) => {
            if (!o?.create) {
              if (window.__dir.has(name)) return {};
              throw Object.assign(new Error('nf'), { name: 'NotFoundError' });
            }
            window.__dir.add(name);
            return { createWritable: async () => writable('dir:' + name) };
          },
        });
      });
      const fp = await receiverPage(ctx, base, link);
      await fp.waitForSelector('#l-files li', { timeout: 40000 });
      await fp.locator('#l-files li').nth(0).locator('button').click();
      await fp.waitForFunction(() => document.getElementById('l-note-text').textContent.startsWith('Done'));
      assert.deepEqual(await fp.evaluate(() => window.__picked), ['photo.bin'], 'окно выбора открыто с именем файла без каталога');
      await fp.locator('#l-all').click();
      await fp.waitForFunction(() => document.getElementById('l-note-text').textContent === 'Done — saved 2 files', null, { timeout: 30000 });
      const saved = await fp.evaluate(() => window.__saved);
      const want = (f) => `${sha(f.data)}:${f.size}`;
      assert.equal(saved['save:photo.bin'], want(FILES[0]));
      assert.equal(saved['dir:photo (2).bin'], want(FILES[0]), 'существующий photo.bin не затёрт');
      assert.equal(saved['dir:' + FILES[1].saved], want(FILES[1]));
      assert.deepEqual(fp.errors, []);
      await ctx.close();
    });

    // 6. Большой файл без showSaveFilePicker — предупреждение (размер подставляем из hello).
    await step('файл больше 1 ГБ без showSaveFilePicker: предупреждение, «Отмена», «Всё равно»', async () => {
      const big = newLink();
      const ctx = await browser.newContext({ acceptDownloads: true });
      await ctx.addInitScript(() => { delete window.showSaveFilePicker; delete window.showDirectoryPicker; });
      const bigSender = await senderCtx.newPage();
      await bigSender.goto(base + '/__sender');
      await startSender(bigSender, big, { helloSizes: { 1: 2 * 1024 ** 3 } }); // в списке — 2 ГБ
      const bp = await receiverPage(ctx, base, big);
      await bp.waitForSelector('#l-files li', { timeout: 40000 });
      await bp.locator('#l-files li').nth(1).locator('button').click();
      await bp.waitForSelector('#l-warn:not([hidden])');
      assert.match(await text(bp, '#l-warn-text'), /^Large file \(2\.0 GB\).*Better open this link on a computer\.$/);
      assert.equal(await bp.locator('#l-prog').isHidden(), true, 'до подтверждения ничего не качается');
      await bp.screenshot({ path: path.join(SHOTS, 'warn-light.png'), fullPage: true });
      await bp.locator('#warn-no').click();
      assert.equal(await bp.locator('#l-warn').isHidden(), true);
      await bp.locator('#l-all').click();
      await bp.waitForSelector('#l-warn:not([hidden])');
      const got = [];
      bp.on('download', (d) => got.push(d));
      await bp.locator('#warn-go').click();
      await bp.waitForFunction(() => document.getElementById('l-note-text').textContent === 'Done — saved 2 files', null, { timeout: 30000 });
      await new Promise((r) => setTimeout(r, 300));
      assert.equal(got.length, 2);
      await bp.close();
      await bigSender.evaluate(() => { window.__s.stop = true; });
      await bigSender.close();
      await ctx.close();
    });

    // 7. Ответ приходит не сразу: 204 — опрос повторяется.
    await step('ответ позже, чем «wait»: на 204 страница опрашивает снова', async () => {
      const slowLink = newLink();
      const slowSender = await senderCtx.newPage();
      await slowSender.goto(base + '/__sender');
      await startSender(slowSender, slowLink, { answerDelay: 2500 });
      rooms.get(slowLink.room).capWait = 1;
      const ctx = await browser.newContext();
      const p = await receiverPage(ctx, base, slowLink);
      await p.waitForSelector('#l-files li', { timeout: 40000 });
      assert.ok(rooms.get(slowLink.room).polls >= 2, 'опросов ответа: ' + rooms.get(slowLink.room).polls);
      assert.deepEqual(p.errors, []);
      await slowSender.evaluate(() => { window.__s.stop = true; });
      await ctx.close();
      await slowSender.close();
    });

    // 8. Потерянное hello (Chromium теряет данные, посланные в миг открытия канала): страница просит его снова.
    await step('hello потерялось: страница шлёт {"t":"ready"} и получает список', async () => {
      const lostLink = newLink();
      const lostSender = await senderCtx.newPage();
      await lostSender.goto(base + '/__sender');
      await startSender(lostSender, lostLink, { noHello: true });
      const ctx = await browser.newContext();
      const p = await receiverPage(ctx, base, lostLink);
      await p.waitForSelector('#l-files li', { timeout: 40000 });
      assert.ok((await lostSender.evaluate(() => window.__s.ready)) >= 1);
      assert.deepEqual(p.errors, []);
      await lostSender.evaluate(() => { window.__s.stop = true; });
      await ctx.close();
      await lostSender.close();
    });

    // 9. Ошибочные ссылки и ответы.
    const errCtx = await browser.newContext({ viewport: { width: 360, height: 800 } });
    await step('неверная ссылка: «Ссылка неверна или неполная», без запросов к службе', async () => {
      const bad = await errCtx.newPage();
      let apiCalls = 0;
      bad.on('request', (r) => r.url().includes('/share/api/') && apiCalls++);
      await bad.goto(`${base}/share/ru/get/?api=${encodeURIComponent(base + '/share/api/v1')}#abc.def`);
      await bad.waitForSelector('#v-error:not([hidden])');
      assert.equal(await text(bad, '#e-title'), 'Ссылка неверна или неполная');
      assert.equal(await bad.locator('#e-retry').isHidden(), true);
      assert.equal(apiCalls, 0);
      await bad.goto(`${base}/share/get/`);
      await bad.waitForSelector('#v-error:not([hidden])');
      assert.equal(await text(bad, '#e-title'), 'The link is wrong or incomplete');
      await bad.close();
    });
    await step('нет такой комнаты (404): «Ссылка устарела»', async () => {
      const p = await receiverPage(errCtx, base, newLink());
      await p.waitForSelector('#v-error:not([hidden])', { timeout: 20000 });
      assert.equal(await text(p, '#e-title'), 'The link has expired');
      assert.equal(await p.locator('#e-install').getAttribute('href'), '/share/');
      await p.screenshot({ path: path.join(SHOTS, 'error-expired-dark.png'), fullPage: true });
      await p.close();
    });
    await step('ответ не расшифровался: «Ссылка неверна»', async () => {
      const l = newLink();
      await errCtx.request.put(`${base}/share/api/v1/rooms/${l.room}`, { headers: { 'X-Owner': l.owner }, data: { ttl: 60 } });
      rooms.get(l.room).autoAnswer = b64u(crypto.randomBytes(40));
      const p = await receiverPage(errCtx, base, l);
      await p.waitForSelector('#v-error:not([hidden])', { timeout: 20000 });
      assert.equal(await text(p, '#e-title'), 'The link is wrong or incomplete');
      await p.close();
    });
    await errCtx.close();

    await sender.evaluate(() => { window.__s.stop = true; });
    assert.deepEqual(await sender.evaluate(() => window.__s.errors), []);
  } catch (e) {
    failures.push(e);
    // Для разбора провала: что видит получатель и отправитель.
    for (const c of browser.contexts()) {
      for (const p of c.pages()) {
        if (p.url().endsWith('/__sender')) {
          console.error('--- sender', await p.evaluate(() => window.__s && ({ errors: window.__s.errors, langs: window.__s.langs, log: window.__s.log, chans: window.__s.chans.map((x) => x.readyState), pcs: window.__s.pcs.map((x) => x.connectionState) })).catch(() => '?'));
        }
      }
    }
    for (const c of browser.contexts()) {
      for (const p of c.pages()) {
        if (p.url().includes('/get/')) console.error('--- ' + p.url().slice(0, 60) + '\n' + await p.locator('#rx').innerText().catch(() => '?'), p.errors, await p.evaluate(() => window.__rx).catch(() => '?'));
      }
    }
  } finally {
    await browser.close();
    server.close();
    server.closeAllConnections?.();
  }
  if (failures.length) {
    console.error('\nПровал:', failures[0]);
    process.exitCode = 1;
  } else {
    console.log(`\nВсё прошло. Снимки экрана: ${SHOTS}`);
  }
}

main().then(() => process.exit(process.exitCode ?? 0));
