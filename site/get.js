// Страница получателя /share/get/ (docs/WEB_P2P.md, разделы 1, 3–6).
// Вставляется в страницу build_share_site.py как есть: __LANG__ и __STRINGS__ подставляет сборка,
// строки комментариев на отдельных строках из страницы убираются. Сторонних скриптов нет.
(() => {
'use strict';

const LANG = '__LANG__';
const S = __STRINGS__;

// Адрес службы связи (раздел 2). Для проверки на своём компьютере его можно заменить параметром
// ?api=…, но только когда страница открыта с localhost / 127.0.0.1.
const API_BASE = (() => {
  const h = location.hostname;
  if (h === 'localhost' || h === '127.0.0.1') {
    const q = new URLSearchParams(location.search).get('api');
    if (q) return q.replace(/\/+$/, '');
  }
  return '/share/api/v1';
})();

const STUN = ['stun:stun.l.google.com:19302', 'stun:stun.cloudflare.com:3478'];
const GATHER_MS = 4000;   // сбор кандидатов ICE — не дольше
const ANSWER_MS = 120000; // сколько ждём ответ устройства
const OPEN_MS = 20000;    // канал должен открыться за это время после ответа
const HELLO_MS = 20000;   // и столько же ждём список файлов после открытия канала
const BIG = 1024 * 1024 * 1024;
const CAN_SAVE = typeof window.showSaveFilePicker === 'function';
const CAN_DIR = CAN_SAVE && typeof window.showDirectoryPicker === 'function';
// Что показывать на экранах ошибок.
const RETRY = ['silent', 'direct', 'lost', 'net', 'busy'];
const INSTALL = ['bad', 'expired', 'direct'];

const $ = (id) => document.getElementById(id);
const enc = new TextEncoder();
const dec = new TextDecoder();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const fmt = (s, v) => s.replace(/\{(\w+)\}/g, (m, k) => (k in v ? String(v[k]) : m));

class Failure extends Error {
  constructor(code) {
    super(code);
    this.code = code;
  }
}

// ── base64url и шифрование (раздел 3) ──

function b64uEnc(bytes) {
  const u = new Uint8Array(bytes);
  let s = '';
  for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode.apply(null, u.subarray(i, i + 0x8000));
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function b64uDec(str) {
  if (!/^[A-Za-z0-9_-]*$/.test(str)) throw new Error('base64url');
  let s = str.replace(/-/g, '+').replace(/_/g, '/');
  while (s.length % 4) s += '=';
  const bin = atob(s);
  const u = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u[i] = bin.charCodeAt(i);
  return u;
}

// Ссылка: #<room>.<key>; room — 16 байт (22 символа), key — 32 байта (43 символа).
function parseLink() {
  const m = /^#([A-Za-z0-9_-]{22})\.([A-Za-z0-9_-]{43})$/.exec(location.hash);
  if (!m) return null;
  try {
    if (b64uDec(m[1]).length !== 16) return null;
    const key = b64uDec(m[2]);
    return key.length === 32 ? { room: m[1], key } : null;
  } catch (e) {
    return null;
  }
}

let cryptoKey = null;

async function seal(obj, aad) {
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  const ct = new Uint8Array(await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv: nonce, additionalData: enc.encode(aad) }, cryptoKey, enc.encode(JSON.stringify(obj))));
  const out = new Uint8Array(12 + ct.length);
  out.set(nonce);
  out.set(ct, 12);
  return b64uEnc(out);
}

async function unseal(msg, aad) {
  const b = b64uDec(msg);
  if (b.length < 12 + 16) throw new Error('short');
  const pt = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: b.subarray(0, 12), additionalData: enc.encode(aad) }, cryptoKey, b.subarray(12));
  return JSON.parse(dec.decode(pt));
}

// ── служба связи ──

let ctl = null; // отменяет все запросы текущей попытки

async function api(method, path, body, ms) {
  const c = new AbortController();
  const master = ctl;
  const onAbort = () => c.abort();
  if (master) master.signal.addEventListener('abort', onAbort);
  const timer = setTimeout(() => c.abort(), ms || 15000);
  try {
    const res = await fetch(API_BASE + path, {
      method,
      cache: 'no-store',
      referrerPolicy: 'no-referrer',
      signal: c.signal,
      headers: body ? { 'Content-Type': 'application/json' } : undefined,
      body: body ? JSON.stringify(body) : undefined,
    });
    let json = null;
    if (res.status === 200 || res.status === 201) {
      try { json = await res.json(); } catch (e) { json = null; }
    }
    return { status: res.status, json };
  } finally {
    clearTimeout(timer);
    if (master) master.signal.removeEventListener('abort', onAbort);
  }
}

// ── экраны ──

let view = '';

function show(v) {
  view = v;
  for (const id of ['connect', 'list', 'error']) $('v-' + id).hidden = id !== v;
}

function setConnect(title, sub) {
  $('c-title').textContent = title;
  $('c-sub').textContent = sub;
}

function fail(kind) {
  run++; // все ещё идущие шаги подключения и загрузки становятся устаревшими
  teardown();
  const e = S.err[kind] || S.err.net;
  $('e-title').textContent = e.t;
  $('e-text').textContent = e.p;
  const tips = $('e-tips');
  tips.textContent = '';
  for (const t of e.tips || []) {
    const li = document.createElement('li');
    li.textContent = t;
    tips.append(li);
  }
  tips.hidden = !(e.tips && e.tips.length);
  $('e-retry').hidden = !RETRY.includes(kind);
  $('e-install').hidden = !INSTALL.includes(kind);
  show('error');
}

function fmtSize(n) {
  const u = S.units;
  if (n < 1024) return n + ' ' + u[0];
  let v = n / 1024;
  let i = 1;
  while (v >= 1024 && i < 3) {
    v /= 1024;
    i++;
  }
  const s = v.toFixed(1);
  return (LANG === 'ru' ? s.replace('.', ',') : s) + ' ' + u[i];
}

function plural(n, forms) {
  if (LANG === 'ru') {
    const a = n % 10;
    const b = n % 100;
    if (a === 1 && b !== 11) return forms[0];
    if (a >= 2 && a <= 4 && (b < 12 || b > 14)) return forms[1];
    return forms[2];
  }
  return n === 1 ? forms[0] : forms[1];
}

const countOf = (n) => n + ' ' + plural(n, S.files_n);

// Имя для сохранения: без каталогов и символов, недопустимых в именах файлов.
function safeName(name) {
  const b = String(name).split(/[\\/]/).pop().replace(/[\u0000-\u001f<>:"|?*]/g, '_').trim();
  return b && b !== '.' && b !== '..' ? b : 'file';
}

// ── подключение (раздел 4) ──

let run = 0;      // номер попытки: устаревшие асинхронные шаги сравнивают его и тихо выходят
let pc = null;
let dc = null;
let timers = [];
let opened = false;
let hello = null;
let files = [];
let busy = false; // идёт скачивание или выбор места сохранения
let cur = null;   // передача, которая идёт сейчас
let queue = null;
let pending = null; // скачивание, ждущее ответа на предупреждение о большом файле

function later(fn, ms) {
  timers.push(setTimeout(fn, ms));
}

function teardown() {
  for (const t of timers) {
    clearTimeout(t);
    clearInterval(t);
  }
  timers = [];
  if (ctl) ctl.abort();
  ctl = null;
  if (cur) {
    const c = cur;
    cur = null;
    c.sink.abort();
  }
  queue = null;
  pending = null;
  busy = false;
  if (dc) {
    dc.onopen = dc.onclose = dc.onmessage = dc.onerror = null;
    try { dc.close(); } catch (e) { /* уже закрыт */ }
  }
  if (pc) {
    pc.onconnectionstatechange = pc.oniceconnectionstatechange = null;
    try { pc.close(); } catch (e) { /* уже закрыт */ }
  }
  dc = pc = null;
}

function gathered(peer, ms) {
  return new Promise((resolve) => {
    if (peer.iceGatheringState === 'complete') return resolve();
    const done = () => {
      clearTimeout(t);
      peer.removeEventListener('icegatheringstatechange', on);
      resolve();
    };
    const on = () => { if (peer.iceGatheringState === 'complete') done(); };
    const t = setTimeout(done, ms);
    peer.addEventListener('icegatheringstatechange', on);
  });
}

async function connect() {
  const my = ++run;
  teardown();
  opened = false;
  hello = null;
  files = [];
  resetList();
  setConnect(S.connecting, S.c_wait);
  show('connect');
  const alive = () => my === run;
  ctl = new AbortController();
  try {
    const peer = new RTCPeerConnection({ iceServers: [{ urls: STUN }] });
    pc = peer;
    const chan = peer.createDataChannel('files', { ordered: true });
    dc = chan;
    chan.binaryType = 'arraybuffer';
    chan.onopen = () => {
      if (!alive()) return;
      opened = true;
      // Chromium теряет сообщение, которое устройство отправило в тот же миг, как открылся канал
      // (в проверке — до 12% соединений), а hello шлётся именно так. Не дождавшись его, просим
      // повторить: {"t":"ready"}; приложение, не знающее этой команды, её игнорирует.
      const nudge = () => {
        if (!alive() || hello) return;
        try { chan.send('{"t":"ready"}'); } catch (e) { /* канал закрыт */ }
        later(nudge, 2000);
      };
      later(nudge, 1000);
      later(() => { if (alive() && !hello) fail('silent'); }, HELLO_MS);
    };
    chan.onclose = () => { if (alive()) fail(opened ? 'lost' : 'direct'); };
    chan.onmessage = (ev) => {
      if (!alive()) return;
      if (typeof ev.data === 'string') {
        let m = null;
        try { m = JSON.parse(ev.data); } catch (e) { return; }
        onCtl(m);
      } else {
        onData(ev.data);
      }
    };
    const down = () => {
      if (alive() && (peer.connectionState === 'failed' || peer.iceConnectionState === 'failed')) {
        fail(opened ? 'lost' : 'direct');
      }
    };
    peer.onconnectionstatechange = down;
    peer.oniceconnectionstatechange = down;

    await peer.setLocalDescription(await peer.createOffer());
    await gathered(peer, GATHER_MS);
    if (!alive()) return;
    const room = link.room;
    const msg = await seal({ sdp: peer.localDescription.sdp, lang: LANG }, 'ogshare-v1|offer|' + room);
    const posted = await api('POST', '/rooms/' + room + '/offers', { msg });
    if (!alive()) return;
    if (posted.status === 404) throw new Failure('expired');
    if (posted.status === 429) throw new Failure('busy');
    if (posted.status !== 201 || !posted.json || typeof posted.json.sid !== 'string') throw new Failure('net');
    const sid = posted.json.sid;

    // Долгий опрос: 204 — ответа ещё нет, спрашиваем снова.
    const deadline = Date.now() + ANSWER_MS;
    let answer = null;
    let errors = 0;
    while (answer === null) {
      if (Date.now() > deadline) throw new Failure('silent');
      const t0 = Date.now();
      let r = null;
      try {
        r = await api('GET', '/rooms/' + room + '/answers/' + sid + '?wait=25', null, 35000);
      } catch (e) {
        if (!alive()) return;
        if (++errors >= 3) throw new Failure('net');
        await sleep(1500);
        continue;
      }
      if (!alive()) return;
      if (r.status === 200 && r.json && typeof r.json.msg === 'string') {
        answer = r.json.msg;
      } else if (r.status === 204) {
        errors = 0;
        if (Date.now() - t0 < 1000) await sleep(1000);
      } else if (r.status === 404) {
        throw new Failure('expired');
      } else if (++errors >= 3) {
        throw new Failure('net');
      } else {
        await sleep(1500);
      }
    }
    if (!alive()) return;

    let sdp = null;
    try {
      const a = await unseal(answer, 'ogshare-v1|answer|' + room + '|' + sid);
      if (a && typeof a.sdp === 'string') sdp = a.sdp;
    } catch (e) { /* не расшифровалось */ }
    if (sdp === null) throw new Failure('bad');
    setConnect(S.connecting, S.c_link);
    try {
      await peer.setRemoteDescription({ type: 'answer', sdp });
    } catch (e) {
      if (!alive()) return;
      throw new Failure('direct');
    }
    if (!alive()) return;
    later(() => { if (alive() && !opened) fail('direct'); }, OPEN_MS);
  } catch (e) {
    if (!alive()) return;
    fail(e instanceof Failure ? e.code : 'net');
  }
}

// ── список файлов ──

function resetList() {
  $('l-files').textContent = '';
  $('l-prog').hidden = true;
  $('l-warn').hidden = true;
  note('');
}

function gotHello(m) {
  const list = Array.isArray(m.files) ? m.files : [];
  files = list
    .filter((f) => f && Number.isInteger(f.i) && typeof f.name === 'string' && Number.isFinite(f.size) && f.size >= 0)
    .map((f) => ({ i: f.i, name: f.name, size: f.size }));
  hello = { name: typeof m.name === 'string' ? m.name : '', expires: Number(m.expires) || 0 };
  $('l-from').textContent = fmt(S.from_, { name: hello.name || S.device });
  updateValid();
  timers.push(setInterval(updateValid, 15000));
  const ul = $('l-files');
  ul.textContent = '';
  for (const f of files) {
    const li = document.createElement('li');
    const n = document.createElement('div');
    n.className = 'n';
    n.textContent = f.name;
    const s = document.createElement('div');
    s.className = 's';
    s.textContent = fmtSize(f.size);
    const b = document.createElement('button');
    b.type = 'button';
    b.className = files.length > 1 ? 'b t' : 'b';
    b.textContent = S.download;
    b.addEventListener('click', () => clickOne(f));
    li.append(n, s, b);
    ul.append(li);
  }
  const all = $('l-all');
  all.hidden = files.length < 2;
  const total = files.reduce((a, f) => a + f.size, 0);
  all.textContent = fmt(S.dl_all, { count: countOf(files.length), size: fmtSize(total) });
  $('l-empty').hidden = files.length > 0;
  refresh();
  show('list');
}

function updateValid() {
  if (!hello || !hello.expires) {
    $('l-valid').textContent = '';
    return;
  }
  const left = hello.expires * 1000 - Date.now();
  $('l-valid').textContent = left > 0 ? fmt(S.valid, { n: Math.max(1, Math.ceil(left / 60000)) }) : S.valid_over;
}

function refresh() {
  for (const b of $('v-list').querySelectorAll('.b')) {
    if (b.id !== 'p-cancel') b.disabled = busy;
  }
}

function note(text, kind) {
  $('l-note').hidden = !text;
  $('l-note').dataset.k = kind || '';
  $('l-note-text').textContent = text || '';
}

// ── сообщения канала (раздел 5) ──

function sendCancel() {
  try { dc.send('{"t":"cancel"}'); } catch (e) { /* канал закрыт */ }
}

// Заканчивает текущую передачу: ошибкой или (без аргумента) успехом.
function settle(err) {
  const c = cur;
  if (!c) return;
  cur = null;
  if (err) {
    c.sink.abort();
    c.reject(err);
  } else {
    c.resolve();
  }
}

function onCtl(m) {
  if (!m || typeof m !== 'object') return;
  if (m.t === 'hello') {
    if (!hello) gotHello(m);
    return;
  }
  if (m.t === 'error' && m.code === 'expired') {
    fail('expired');
    return;
  }
  const c = cur;
  if (!c) return;
  if (m.t === 'file' && c.phase === 'wait' && m.i === c.i) {
    const size = Number(m.size);
    if (!(size >= 0) || Number(m.offset || 0) !== 0) {
      sendCancel();
      settle(new Failure('size'));
      return;
    }
    c.size = size;
    c.phase = 'data';
    c.samples = [[performance.now(), 0]];
    paint(true);
  } else if (m.t === 'end' && c.phase === 'data' && m.i === c.i) {
    complete(c);
  } else if (m.t === 'error' && (m.i === c.i || m.i === undefined)) {
    settle(new Failure(['not_found', 'read'].includes(m.code) ? m.code : 'other'));
  }
}

function onData(buf) {
  const c = cur;
  // Куски отменённого файла могут дойти уже после отмены — до заголовка `file` их отбрасываем.
  if (!c || c.phase !== 'data' || !(buf instanceof ArrayBuffer)) return;
  c.got += buf.byteLength;
  if (c.got > c.size) {
    sendCancel();
    settle(new Failure('size'));
    return;
  }
  c.sink.push(buf);
  paint(false);
}

async function complete(c) {
  if (c.got !== c.size) {
    settle(new Failure('size'));
    return;
  }
  cur = null; // дальше сообщений по этому файлу не ждём
  try {
    await c.sink.finish();
    c.resolve();
  } catch (e) {
    c.reject(new Failure('write'));
  }
}

function transfer(f, sink, step) {
  return new Promise((resolve, reject) => {
    const c = { f, i: f.i, sink, size: f.size, got: 0, phase: 'wait', step, resolve, reject, samples: [], at: 0 };
    sink.onerror = () => {
      if (cur === c) {
        sendCancel();
        settle(new Failure('write'));
      }
    };
    cur = c;
    paint(true);
    try {
      dc.send(JSON.stringify({ t: 'get', i: f.i, offset: 0 }));
    } catch (e) {
      cur = null;
      sink.abort();
      reject(new Failure('lost'));
    }
  });
}

// ── ход загрузки ──

function paint(force) {
  const c = cur;
  if (!c) return;
  const now = performance.now();
  if (!force && now - c.at < 250) return;
  c.at = now;
  $('p-step').hidden = !c.step;
  if (c.step) $('p-step').textContent = fmt(S.step, c.step);
  $('p-name').textContent = c.f.name;
  const pct = c.size > 0 ? Math.min(100, (c.got / c.size) * 100) : 0;
  $('p-fill').style.width = pct + '%';
  $('p-bar').setAttribute('aria-valuenow', String(Math.round(pct)));
  if (c.phase === 'wait') {
    $('p-text').textContent = S.p_wait;
    return;
  }
  c.samples.push([now, c.got]);
  while (c.samples.length > 2 && now - c.samples[0][0] > 3000) c.samples.shift();
  const [t0, b0] = c.samples[0];
  const dt = (now - t0) / 1000;
  let text = fmt(S.of, { done: fmtSize(c.got), total: fmtSize(c.size) });
  if (dt >= 0.3) text += ' · ' + fmtSize(Math.round((c.got - b0) / dt)) + S.per_s;
  $('p-text').textContent = text;
}

// ── сохранение (раздел 6) ──

function saveBlob(parts, name) {
  const url = URL.createObjectURL(new Blob(parts, { type: 'application/octet-stream' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.rel = 'noopener';
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60000);
}

// Свободное имя в выбранной папке: не затираем уже лежащие там файлы.
async function freeHandle(dir, name, used) {
  const dot = name.lastIndexOf('.');
  const base = dot > 0 ? name.slice(0, dot) : name;
  const ext = dot > 0 ? name.slice(dot) : '';
  for (let k = 1; k < 1000; k++) {
    const n = k === 1 ? name : base + ' (' + k + ')' + ext;
    if (used.has(n.toLowerCase())) continue;
    try {
      await dir.getFileHandle(n);
    } catch (e) {
      if (e && e.name === 'NotFoundError') {
        used.add(n.toLowerCase());
        return dir.getFileHandle(n, { create: true });
      }
      throw e;
    }
  }
  throw new Error('names');
}

// Приёмник данных файла: поток в выбранный файл или куски в памяти, которые в конце станут Blob.
async function openSink(f, target, all, used) {
  const name = safeName(f.name);
  if (!target) {
    const parts = [];
    return {
      onerror: null,
      push(b) { parts.push(b); },
      async finish() { saveBlob(parts, name); parts.length = 0; },
      abort() { parts.length = 0; },
    };
  }
  const handle = all ? await freeHandle(target, name, used) : target;
  const w = await handle.createWritable();
  let chain = Promise.resolve();
  let failed = false;
  const s = {
    onerror: null,
    // Записи идут строго по очереди; ошибка одной отменяет остальные.
    push(b) {
      chain = chain.then(() => (failed ? null : w.write(b))).catch((e) => {
        if (failed) return;
        failed = true;
        if (s.onerror) s.onerror(e);
      });
    },
    async finish() {
      await chain;
      if (failed) throw new Error('write');
      await w.close();
    },
    abort() {
      failed = true;
      chain.then(() => w.abort()).catch(() => {});
    },
  };
  return s;
}

function clickOne(f) {
  if (busy) return;
  if (!CAN_SAVE && f.size > BIG) {
    askBig([f], false);
    return;
  }
  start([f], false);
}

function clickAll() {
  if (busy) return;
  if (!CAN_DIR && files.some((f) => f.size > BIG)) {
    askBig(files, true);
    return;
  }
  start(files, true);
}

function askBig(list, all) {
  pending = { list, all };
  $('l-warn-text').textContent = fmt(S.big, { size: fmtSize(Math.max(...list.map((f) => f.size))) });
  $('l-warn').hidden = false;
}

async function start(list, all) {
  if (busy || !list.length) return;
  busy = true;
  refresh();
  const my = run;
  // Место сохранения спрашиваем сразу в обработчике клика: после первого await жест пользователя
  // уже не действует, и браузер отказал бы в окне выбора.
  let target = null;
  try {
    if (CAN_SAVE && !all) target = await window.showSaveFilePicker({ suggestedName: safeName(list[0].name) });
    else if (CAN_DIR && all) target = await window.showDirectoryPicker({ mode: 'readwrite' });
  } catch (e) {
    if (my !== run) return;
    if (e && e.name === 'AbortError') {
      busy = false;
      refresh();
      return;
    }
    target = null; // окно выбора недоступно — сохраним через Blob
  }
  if (my !== run) return;
  const q = { stop: false, err: null, file: null, saved: 0 };
  queue = q;
  note('');
  $('l-warn').hidden = true;
  $('l-prog').hidden = false;
  const used = new Set();
  for (let n = 0; n < list.length && !q.stop; n++) {
    const f = list[n];
    q.file = f;
    let sink = null;
    try {
      sink = await openSink(f, target, all, used);
      if (my !== run) {
        sink.abort();
        return;
      }
      if (q.stop) {
        sink.abort();
        break;
      }
      await transfer(f, sink, all ? { n: n + 1, total: list.length } : null);
      if (my !== run) return;
      q.saved++;
    } catch (e) {
      if (my !== run) return;
      q.err = e instanceof Failure ? e : new Failure('write');
      break;
    }
  }
  queue = null;
  busy = false;
  $('l-prog').hidden = true;
  refresh();
  if (q.err && q.err.code !== 'cancel') {
    const why = S.ferr_why[q.err.code] || S.ferr_why.other;
    note(fmt(S.ferr, { name: q.file.name }) + ' ' + why, 'err');
  } else if (q.err || q.stop) {
    note(S.cancelled);
  } else {
    note(all ? fmt(S.done_all, { count: countOf(q.saved) }) : fmt(S.done_one, { name: safeName(list[0].name) }), 'ok');
  }
}

// ── запуск ──

let link = null;

function init() {
  $('rx').hidden = false;
  // Переключатель языка в шапке не должен терять ссылку после «#».
  for (const a of document.querySelectorAll('.langs a')) a.hash = location.hash;
  window.addEventListener('hashchange', () => location.reload());
  $('e-retry').addEventListener('click', connect);
  $('l-all').addEventListener('click', clickAll);
  $('p-cancel').addEventListener('click', () => {
    if (queue) queue.stop = true;
    if (cur) {
      sendCancel();
      settle(new Failure('cancel'));
    }
  });
  $('warn-go').addEventListener('click', () => {
    const p = pending;
    pending = null;
    $('l-warn').hidden = true;
    if (p) start(p.list, p.all);
  });
  $('warn-no').addEventListener('click', () => {
    pending = null;
    $('l-warn').hidden = true;
  });
  if (!window.RTCPeerConnection || !window.fetch || !window.crypto || !crypto.subtle) {
    fail('unsupported');
    return;
  }
  link = parseLink();
  if (!link) {
    fail('bad');
    return;
  }
  crypto.subtle.importKey('raw', link.key, 'AES-GCM', false, ['encrypt', 'decrypt']).then((k) => {
    cryptoKey = k;
    connect();
  }, () => fail('bad'));
}

init();
})();
