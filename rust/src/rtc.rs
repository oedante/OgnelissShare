//! Файлы без приложения через интернет: устройство → браузер по WebRTC.
//!
//! Формат ссылки, шифрование сообщений, API службы связи и протокол канала
//! описаны в `docs/WEB_P2P.md`; здесь сторона приложения. «Комната» на службе
//! связи — почтовый ящик: браузер кладёт туда зашифрованное предложение (SDP),
//! приложение забирает его, отвечает и дальше говорит с браузером напрямую.
//!
//! WebRTC — str0m (sans-IO): сокеты, таймеры и порядок вызовов ведём сами.
//! Один браузер — одна задача [`Peer`] со своими UDP-сокетами, по одному на
//! адрес интерфейса: адрес из STUN относится к тому же порту, с которого идут
//! проверки связности.

use std::collections::{HashSet, VecDeque};
use std::io::{self, SeekFrom};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{Context, anyhow, bail, ensure};
use data_encoding::BASE64URL_NOPAD as B64;
use serde::Deserialize;
use serde_json::{Value, json};
use str0m::change::SdpOffer;
use str0m::channel::ChannelId;
use str0m::net::{Protocol, Receive, Transmit};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufReader};
use tokio::net::{UdpSocket, lookup_host};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{sleep, sleep_until, timeout, timeout_at};
use tokio_util::sync::CancellationToken;

use crate::events::Events;
use crate::util::now_unix;
use crate::web::WebFile;

/// Сколько получателей обслуживаем одновременно.
const MAX_PEERS: usize = 4;
/// Сколько сокетов (адресов интерфейсов) открываем на одного получателя.
const MAX_SOCKETS: usize = 8;
/// Размер куска файла в канале.
const CHUNK: usize = 16 * 1024;
/// Пока в буфере канала не меньше этого, новые куски не пишем.
const HIGH_WATER: usize = 1 << 20;
/// Когда буфер опустел до этого, str0m сообщает `ChannelBufferedAmountLow`.
const LOW_WATER: usize = 256 * 1024;
/// Канал `files` должен открыться за это время после ответа.
const OPEN_TIMEOUT: Duration = Duration::from_secs(30);
/// Пауза перед повтором, когда служба связи недоступна.
const RETRY: Duration = Duration::from_secs(15);
/// Как долго службе связи ждать новых предложений (≤ 30 по протоколу).
const POLL_WAIT: u64 = 25;
/// Как часто продлевать комнату, если раздача длиннее часа.
const REFRESH: Duration = Duration::from_secs(600);
/// Срок ожидания STUN: и на DNS, и на ответ сервера.
const STUN_WAIT: Duration = Duration::from_millis(1500);
/// Какой срок комнаты (секунды) принимает служба связи: от минуты до часа.
const ROOM_TTL: std::ops::RangeInclusive<u64> = 60..=3600;

const STATUS_CONNECTING: u8 = 0;
const STATUS_READY: u8 = 1;
const STATUS_OFFLINE: u8 = 2;

type IpSource = Arc<dyn Fn() -> Vec<IpAddr> + Send + Sync>;
/// Пакет из сети: откуда, на какой локальный адрес, что.
type Packet = (SocketAddr, SocketAddr, Vec<u8>);

/// Где служба связи, страница получателя и STUN-серверы.
#[derive(Clone, Debug)]
pub struct RtcConfig {
    /// Адрес API службы связи, без «/» в конце.
    pub api: String,
    /// Адрес страницы получателя; к нему добавляется `#<room>.<key>`.
    pub page: String,
    /// STUN-серверы «хост:порт» (те же, что у браузера).
    pub stun: Vec<String>,
}

impl Default for RtcConfig {
    fn default() -> Self {
        RtcConfig {
            api: "https://ogneliss.ru/share/api/v1".to_string(),
            page: "https://ogneliss.ru/share/get/".to_string(),
            stun: vec!["stun.l.google.com:19302".to_string(), "stun.cloudflare.com:3478".to_string()],
        }
    }
}

/// Одна раздача в интернет: комната на службе связи и все её получатели.
pub struct Room {
    cfg: RtcConfig,
    share_id: String,
    /// Имя комнаты на службе связи (16 байт, base64url).
    room: String,
    /// Секрет владельца для службы (`X-Owner`).
    owner: String,
    link: String,
    /// Ключ сообщений через службу; в ссылке после «#».
    cipher: Aes256Gcm,
    files: Arc<Vec<WebFile>>,
    /// Конец раздачи, секунды Unix — для `hello`.
    expires: u64,
    deadline: Instant,
    local_ips: IpSource,
    device_name: Mutex<String>,
    events: Arc<Events>,
    cancel: CancellationToken,
    status: AtomicU8,
    downloads: AtomicU32,
    /// Получатели с открытым каналом.
    peers: AtomicUsize,
    /// Занятые места: от принятого предложения до конца задачи получателя.
    active: AtomicUsize,
}

impl Room {
    /// Запустить раздачу: ключи создаются здесь же, сеть — в фоновой задаче.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        cfg: RtcConfig,
        share_id: String,
        device_name: String,
        files: Vec<WebFile>,
        ttl: Duration,
        local_ips: Arc<dyn Fn() -> Vec<IpAddr> + Send + Sync>,
        events: Arc<Events>,
        cancel: CancellationToken,
    ) -> Arc<Room> {
        let room = Room::new(cfg, share_id, device_name, files, ttl, local_ips, events, cancel);
        room.spawn(None);
        room
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        cfg: RtcConfig,
        share_id: String,
        device_name: String,
        files: Vec<WebFile>,
        ttl: Duration,
        local_ips: IpSource,
        events: Arc<Events>,
        cancel: CancellationToken,
    ) -> Arc<Room> {
        let key: [u8; 32] = rand::random();
        let room = B64.encode(&rand::random::<[u8; 16]>());
        let link = format!("{}#{}.{}", cfg.page, room, B64.encode(&key));
        Arc::new(Room {
            owner: B64.encode(&rand::random::<[u8; 16]>()),
            cipher: Aes256Gcm::new(&Key::<Aes256Gcm>::from(key)),
            link,
            room,
            share_id,
            files: Arc::new(files),
            expires: now_unix() + ttl.as_secs(),
            deadline: Instant::now() + ttl,
            local_ips,
            device_name: Mutex::new(device_name),
            events,
            cancel: cancel.child_token(),
            status: AtomicU8::new(STATUS_CONNECTING),
            downloads: AtomicU32::new(0),
            peers: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            cfg,
        })
    }

    /// Фоновая задача комнаты. `backend` задают только тесты.
    fn spawn(self: &Arc<Self>, backend: Option<Backend>) {
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                rt.spawn(self.clone().run(backend));
            }
            Err(_) => {
                self.log("нет среды выполнения, раздача не запущена");
                self.set_status(STATUS_OFFLINE);
            }
        }
    }

    /// Ссылка для получателя: `<страница>#<room>.<key>`.
    pub fn link(&self) -> &str {
        &self.link
    }

    /// «connecting» до первого успешного `PUT`, затем «ready»; «offline», пока служба недоступна.
    pub fn status(&self) -> &'static str {
        match self.status.load(Ordering::SeqCst) {
            STATUS_READY => "ready",
            STATUS_OFFLINE => "offline",
            _ => "connecting",
        }
    }

    /// Сколько раз файл скачали целиком (с нулевого байта до конца), всеми получателями.
    pub fn downloads(&self) -> u32 {
        self.downloads.load(Ordering::SeqCst)
    }

    /// Сколько получателей сейчас на связи.
    pub fn peers(&self) -> usize {
        self.peers.load(Ordering::SeqCst)
    }

    pub fn set_device_name(&self, name: &str) {
        *lock(&self.device_name) = name.to_string();
    }

    /// Остановить: отключить получателей и удалить комнату на службе (фоном, без гарантий).
    pub fn close(&self) {
        self.cancel.cancel();
    }

    fn log(&self, msg: impl std::fmt::Display) {
        self.events.log(format!("rtc {}: {msg}", self.share_id));
    }

    fn set_status(&self, new: u8) {
        if self.status.swap(new, Ordering::SeqCst) != new {
            self.events.changed();
        }
    }

    fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    /// Жизнь комнаты: от `PUT` до `DELETE`.
    async fn run(self: Arc<Self>, backend: Option<Backend>) {
        // Корни Mozilla вшиты в приложение: системное хранилище Android требует отдельной настройки JNI.
        let backend = match backend.map_or_else(|| Backend::http(&self.cfg.api, Default::default()), Ok) {
            Ok(b) => Arc::new(b),
            Err(e) => {
                self.log(format!("нет клиента HTTPS: {e}"));
                self.set_status(STATUS_OFFLINE);
                return;
            }
        };
        tokio::select! {
            _ = self.cancel.cancelled() => {}
            _ = sleep_until(at(self.deadline)) => self.log("раздача истекла"),
            _ = self.serve(&backend) => {}
        }
        // Получатели отключаются по тому же сигналу.
        self.cancel.cancel();
        let gone = timeout(Duration::from_secs(5), backend.delete_room(&self.room, &self.owner)).await;
        self.log(if matches!(gone, Ok(Ok(()))) {
            "комната удалена"
        } else {
            "комната не удалена"
        });
    }

    /// Держим комнату и забираем предложения, пока не остановят.
    async fn serve(self: &Arc<Self>, backend: &Arc<Backend>) {
        let mut after = 0u64;
        let mut seen = HashSet::new();
        let mut need_put = true;
        let mut put_at = Instant::now();
        let mut long = false;
        loop {
            if need_put || (long && put_at.elapsed() >= REFRESH) {
                let left = self.deadline.saturating_duration_since(Instant::now()).as_secs();
                match backend
                    .put_room(&self.room, &self.owner, left.clamp(*ROOM_TTL.start(), *ROOM_TTL.end()))
                    .await
                {
                    Ok(()) => {
                        (need_put, put_at, long) = (false, Instant::now(), left > *ROOM_TTL.end());
                        self.set_status(STATUS_READY);
                    }
                    Err(e) => {
                        self.offline("PUT", e);
                        sleep(RETRY).await;
                        continue;
                    }
                }
            }
            let started = Instant::now();
            match backend.offers(&self.room, &self.owner, after, POLL_WAIT).await {
                Ok(list) => {
                    self.set_status(STATUS_READY);
                    if list.is_empty() && started.elapsed() < Duration::from_secs(1) {
                        sleep(Duration::from_secs(1)).await;
                    }
                    for o in list {
                        after = after.max(o.seq);
                        if seen.insert(o.sid.clone()) {
                            self.on_offer(backend, o);
                        }
                    }
                }
                // Служба забыла комнату (перезапуск): создаём заново, номера предложений пойдут с единицы.
                Err(SigError::Status(404)) => {
                    (need_put, after) = (true, 0);
                    sleep(Duration::from_secs(1)).await;
                }
                Err(e) => {
                    (need_put, after) = (true, 0);
                    self.offline("GET", e);
                    sleep(RETRY).await;
                }
            }
        }
    }

    fn offline(&self, what: &str, e: SigError) {
        if self.status() != "offline" {
            self.log(format!("служба связи недоступна ({what}: {e:?}), повтор через {} с", RETRY.as_secs()));
        }
        self.set_status(STATUS_OFFLINE);
    }

    /// Предложение браузера: расшифровать и отдать получателю. Чужое и битое молча отбрасываем.
    fn on_offer(self: &Arc<Self>, backend: &Arc<Backend>, offer: Offer) {
        let Some(plain) = open(&self.cipher, &format!("ogshare-v1|offer|{}", self.room), &offer.msg) else {
            return;
        };
        #[derive(Deserialize)]
        struct OfferMsg {
            sdp: String,
        }
        let Ok(OfferMsg { sdp }) = serde_json::from_slice(&plain) else { return };
        if self.active.fetch_add(1, Ordering::SeqCst) >= MAX_PEERS {
            self.active.fetch_sub(1, Ordering::SeqCst);
            self.log("все места заняты, предложение отклонено");
            return;
        }
        let slot = Slot { room: self.clone(), connected: false };
        tokio::spawn(run_peer(slot, backend.clone(), offer.sid, sdp));
    }

    fn download_done(&self, name: &str) {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        self.events.emit(json!({"type": "web_download", "share": self.share_id, "name": name}));
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Момент для таймеров tokio.
fn at(i: Instant) -> tokio::time::Instant {
    tokio::time::Instant::from_std(i)
}

// ---------------------------------------------------------------------------
// Шифрование сообщений через службу связи (раздел 3 протокола)
// ---------------------------------------------------------------------------

/// `base64url(nonce ‖ шифротекст+тег)`, AES-256-GCM; `aad` связывает сообщение с комнатой.
fn seal(cipher: &Aes256Gcm, aad: &str, plain: &[u8]) -> Option<String> {
    let nonce: [u8; 12] = rand::random();
    let ct = cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: aad.as_bytes() }).ok()?;
    Some(B64.encode(&[nonce.as_slice(), &ct].concat()))
}

/// Обратное к [`seal`]; `None` — не наше, повреждено или чужой ключ.
fn open(cipher: &Aes256Gcm, aad: &str, msg: &str) -> Option<Vec<u8>> {
    let raw = B64.decode(msg.as_bytes()).ok()?;
    if raw.len() < 12 + 16 {
        return None;
    }
    let (nonce, ct) = raw.split_at(12);
    cipher.decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: aad.as_bytes() }).ok()
}

// ---------------------------------------------------------------------------
// Служба связи (раздел 2 протокола): HTTP; в тестах ещё и почтовый ящик в памяти
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum SigError {
    /// Нет связи, таймаут, мусор в ответе.
    Net,
    /// Служба ответила кодом, отличным от 2xx.
    Status(u16),
}

#[derive(Deserialize)]
struct Offer {
    seq: u64,
    sid: String,
    msg: String,
}

enum Backend {
    Http {
        client: reqwest::Client,
        api: String,
    },
    #[cfg(test)]
    Memory(Arc<mem::MemBox>),
}

impl Backend {
    fn http(api: &str, roots: iroh::tls::CaTlsConfig) -> anyhow::Result<Backend> {
        // Тот же rustls (ring), что у iroh: ни aws-lc, ни OpenSSL.
        let tls = roots.client_config(iroh::tls::default_provider())?;
        let client = reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .connect_timeout(Duration::from_secs(10))
            .pool_idle_timeout(Duration::from_secs(20))
            .build()?;
        Ok(Backend::Http { client, api: api.trim_end_matches('/').to_string() })
    }

    /// `PUT /rooms/{room}`: создать или продлить комнату.
    async fn put_room(&self, room: &str, owner: &str, ttl: u64) -> Result<(), SigError> {
        match self {
            Backend::Http { client, api } => {
                let req = client.put(format!("{api}/rooms/{room}")).header("X-Owner", owner);
                http_call(req.body(json!({"ttl": ttl}).to_string()), 10).await.map(drop)
            }
            #[cfg(test)]
            Backend::Memory(m) => m.put_room(room, owner, ttl),
        }
    }

    /// `DELETE /rooms/{room}`.
    async fn delete_room(&self, room: &str, owner: &str) -> Result<(), SigError> {
        match self {
            Backend::Http { client, api } => {
                http_call(client.delete(format!("{api}/rooms/{room}")).header("X-Owner", owner), 5)
                    .await
                    .map(drop)
            }
            #[cfg(test)]
            Backend::Memory(m) => m.delete_room(room, owner),
        }
    }

    /// `GET /rooms/{room}/offers?after=…&wait=…`: длинный запрос, пустой список — ничего нового.
    async fn offers(&self, room: &str, owner: &str, after: u64, wait: u64) -> Result<Vec<Offer>, SigError> {
        match self {
            Backend::Http { client, api } => {
                let url = format!("{api}/rooms/{room}/offers?after={after}&wait={wait}");
                let body = http_call(client.get(url).header("X-Owner", owner), wait + 10).await?;
                #[derive(Deserialize)]
                struct Offers {
                    offers: Vec<Offer>,
                }
                serde_json::from_slice::<Offers>(&body).map(|o| o.offers).map_err(|_| SigError::Net)
            }
            #[cfg(test)]
            Backend::Memory(m) => m.offers(room, owner, after, wait).await,
        }
    }

    /// `PUT /rooms/{room}/answers/{sid}`.
    async fn put_answer(&self, room: &str, owner: &str, sid: &str, msg: &str) -> Result<(), SigError> {
        match self {
            Backend::Http { client, api } => {
                let req = client.put(format!("{api}/rooms/{room}/answers/{sid}")).header("X-Owner", owner);
                http_call(req.body(json!({"msg": msg}).to_string()), 10).await.map(drop)
            }
            #[cfg(test)]
            Backend::Memory(m) => m.put_answer(room, owner, sid, msg),
        }
    }
}

/// Один запрос; тело ответа при коде 2xx.
async fn http_call(req: reqwest::RequestBuilder, secs: u64) -> Result<Vec<u8>, SigError> {
    let resp = req
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(secs))
        .send()
        .await
        .map_err(|_| SigError::Net)?;
    let code = resp.status().as_u16();
    let body = resp.bytes().await.map_err(|_| SigError::Net)?;
    if (200..300).contains(&code) { Ok(body.to_vec()) } else { Err(SigError::Status(code)) }
}

// ---------------------------------------------------------------------------
// STUN (RFC 5389): наш адрес снаружи NAT для server-reflexive кандидата
// ---------------------------------------------------------------------------

const STUN_COOKIE: [u8; 4] = [0x21, 0x12, 0xA4, 0x42];

/// Binding Request без атрибутов.
fn stun_request(txn: &[u8; 12]) -> [u8; 20] {
    let mut m = [0u8; 20];
    m[0..2].copy_from_slice(&[0x00, 0x01]);
    m[4..8].copy_from_slice(&STUN_COOKIE);
    m[8..20].copy_from_slice(txn);
    m
}

/// Адрес из ответа на Binding Request: XOR-MAPPED-ADDRESS, а у старых серверов MAPPED-ADDRESS.
fn parse_stun_response(buf: &[u8], txn: &[u8; 12]) -> Option<SocketAddr> {
    if buf.len() < 20 || buf[0..2] != [0x01, 0x01] || buf[4..8] != STUN_COOKIE || buf[8..20] != txn[..] {
        return None;
    }
    let body = buf.get(20..20 + usize::from(u16::from_be_bytes([buf[2], buf[3]])))?;
    let (mut p, mut plain) = (0, None);
    while p + 4 <= body.len() {
        let kind = u16::from_be_bytes([body[p], body[p + 1]]);
        let len = usize::from(u16::from_be_bytes([body[p + 2], body[p + 3]]));
        let val = body.get(p + 4..p + 4 + len)?;
        match kind {
            0x0020 => return stun_address(val, true, txn),
            0x0001 => plain = stun_address(val, false, txn),
            _ => {}
        }
        p += 4 + len.div_ceil(4) * 4;
    }
    plain
}

/// Значение атрибута адреса: нулевой байт, семейство, порт, адрес (для XOR — с маской).
fn stun_address(val: &[u8], xor: bool, txn: &[u8; 12]) -> Option<SocketAddr> {
    let mask: Vec<u8> = if xor { [STUN_COOKIE.as_slice(), txn].concat() } else { vec![0; 16] };
    let unmask = |b: &[u8]| -> Vec<u8> { b.iter().zip(&mask).map(|(x, m)| x ^ m).collect() };
    let port = u16::from_be_bytes([*val.get(2)?, *val.get(3)?]) ^ u16::from_be_bytes([mask[0], mask[1]]);
    let ip = match val.get(1)? {
        1 => IpAddr::from(<[u8; 4]>::try_from(unmask(val.get(4..8)?)).ok()?),
        2 => IpAddr::from(<[u8; 16]>::try_from(unmask(val.get(4..20)?)).ok()?),
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// Адреса STUN-серверов (только IPv4): имена разбираем параллельно, каждому не дольше `STUN_WAIT`.
async fn resolve_stun(servers: &[String]) -> Vec<SocketAddr> {
    let mut set = JoinSet::new();
    for s in servers {
        let s = s.clone();
        set.spawn(
            async move { timeout(STUN_WAIT, lookup_host(s)).await.ok()?.ok()?.find(SocketAddr::is_ipv4) },
        );
    }
    let mut out = Vec::new();
    while let Some(r) = set.join_next().await {
        out.extend(r.ok().flatten());
    }
    out
}

/// Спросить у серверов наш адрес снаружи с этого сокета; побеждает первый ответ.
async fn stun_binding(sock: &UdpSocket, servers: &[SocketAddr]) -> Option<SocketAddr> {
    let txn: [u8; 12] = rand::random();
    let req = stun_request(&txn);
    let mut buf = [0u8; 1500];
    // Запрос уходит трижды за `STUN_WAIT`: UDP теряется.
    for _ in 0..3 {
        for s in servers {
            let _ = sock.send_to(&req, s).await;
        }
        let until = Instant::now() + STUN_WAIT / 3;
        while let Ok(Ok((n, _))) = timeout_at(at(until), sock.recv_from(&mut buf)).await {
            if let Some(a) = parse_stun_response(&buf[..n], &txn) {
                return Some(a);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Получатель
// ---------------------------------------------------------------------------

/// Место получателя: освобождается при любом выходе из задачи, даже по панике.
struct Slot {
    room: Arc<Room>,
    connected: bool,
}

impl Slot {
    /// Канал открылся — получатель «на связи».
    fn connect(&mut self) {
        if !self.connected {
            self.connected = true;
            self.room.peers.fetch_add(1, Ordering::SeqCst);
            self.room.events.changed();
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        if self.connected {
            self.room.peers.fetch_sub(1, Ordering::SeqCst);
        }
        self.room.active.fetch_sub(1, Ordering::SeqCst);
        self.room.events.changed();
    }
}

async fn run_peer(slot: Slot, backend: Arc<Backend>, sid: String, sdp: String) {
    let room = slot.room.clone();
    let tag: String = sid.chars().take(6).collect();
    match Peer::connect(slot, &backend, &sid, &sdp).await {
        // В сообщении об ошибке только наш текст: в тексте ошибок str0m бывают куски SDP.
        Ok(peer) => {
            room.log(format!("получатель {tag}: подключён"));
            peer.run().await;
            room.log(format!("получатель {tag}: отключён"));
        }
        Err(e) => room.log(format!("получатель {tag}: {e}")),
    }
}

/// Отдача одного файла.
struct Transfer {
    i: usize,
    /// Размер на диске в момент открытия.
    size: u64,
    /// С какого байта отдаём.
    offset: u64,
    left: u64,
    reader: BufReader<File>,
    stage: Stage,
    /// Кусок, который канал пока не принял.
    pending: Option<Vec<u8>>,
}

#[derive(PartialEq)]
enum Stage {
    Header,
    Data,
    End,
}

/// Команда браузера по каналу.
#[derive(Deserialize)]
struct Command {
    t: String,
    i: Option<u64>,
    offset: Option<u64>,
}

struct Peer {
    room: Arc<Room>,
    slot: Slot,
    rtc: Rtc,
    socks: Vec<(SocketAddr, Arc<UdpSocket>)>,
    /// Читатели сокетов; при выходе из задачи отменяются (JoinSet отменяет задачи при уничтожении).
    _readers: JoinSet<()>,
    rx: mpsc::Receiver<Packet>,
    chan: Option<ChannelId>,
    /// Принятые, но ещё не разобранные команды.
    inbox: VecDeque<Vec<u8>>,
    send: Option<Transfer>,
    /// До этого момента канал должен открыться.
    open_by: Instant,
    done: bool,
}

impl Peer {
    /// Разобрать предложение, собрать кандидатов, отправить ответ службе.
    async fn connect(slot: Slot, backend: &Backend, sid: &str, sdp: &str) -> anyhow::Result<Peer> {
        let room = slot.room.clone();
        let sdp = data_only_sdp(sdp).ok_or_else(|| anyhow!("в предложении не только канал данных"))?;
        let offer = SdpOffer::from_sdp_string(&sdp).map_err(|_| anyhow!("предложение не разобрано"))?;

        let socks = bind_sockets((room.local_ips)()).await;
        ensure!(!socks.is_empty(), "нет адресов для приёма");
        let mut rtc = Rtc::builder()
            .set_crypto_provider(crypto::provider())
            // Канал отдаёт до HIGH_WATER в полёте; запас — на служебные сообщения.
            .set_sctp_max_buffered_amount(HIGH_WATER + 64 * 1024)
            .build(Instant::now());
        for (addr, _) in &socks {
            rtc.add_local_candidate(Candidate::host(*addr, "udp").context("кандидат host")?);
        }
        for (base, mapped) in reflexive(&socks, &room.cfg.stun).await {
            if let Ok(c) = Candidate::server_reflexive(mapped, base, "udp") {
                rtc.add_local_candidate(c);
            }
        }

        let answer = rtc.sdp_api().accept_offer(offer).map_err(|_| anyhow!("предложение отклонено"))?;
        let body = json!({"sdp": answer.to_sdp_string()}).to_string();
        let aad = format!("ogshare-v1|answer|{}|{sid}", room.room);
        let msg = seal(&room.cipher, &aad, body.as_bytes()).ok_or_else(|| anyhow!("ответ не зашифрован"))?;
        let mut sent = backend.put_answer(&room.room, &room.owner, sid, &msg).await;
        for _ in 0..2 {
            if sent.is_ok() {
                break;
            }
            sleep(Duration::from_secs(1)).await;
            sent = backend.put_answer(&room.room, &room.owner, sid, &msg).await;
        }
        if sent.is_err() {
            bail!("ответ не доставлен");
        }

        let (tx, rx) = mpsc::channel(256);
        let mut readers = JoinSet::new();
        for (local, sock) in &socks {
            readers.spawn(read_socket(*local, sock.clone(), tx.clone()));
        }
        Ok(Peer {
            room,
            slot,
            rtc,
            socks,
            _readers: readers,
            rx,
            chan: None,
            inbox: VecDeque::new(),
            send: None,
            open_by: Instant::now() + OPEN_TIMEOUT,
            done: false,
        })
    }

    /// Главный цикл: команды браузера → запись в канал → пакеты и события str0m → ожидание.
    async fn run(mut self) {
        loop {
            while let Some(raw) = self.inbox.pop_front() {
                self.command(&raw).await;
            }
            let more = self.pump().await;
            let next = self.drain();
            if self.finished() {
                break;
            }
            if more || !self.inbox.is_empty() {
                continue;
            }
            let limit =
                if self.chan.is_none() { self.open_by.min(self.room.deadline) } else { self.room.deadline };
            tokio::select! {
                _ = self.room.cancel.cancelled() => {}
                _ = sleep_until(at(limit)) => {}
                _ = sleep_until(at(next)) => {
                    if self.rtc.handle_input(Input::Timeout(Instant::now())).is_err() {
                        self.done = true;
                    }
                }
                // По одному пакету: после каждого вызова str0m надо забрать его вывод (`drain`),
                // иначе очередь DTLS переполняется.
                got = self.rx.recv() => {
                    let Some(packet) = got else { break };
                    self.input(packet);
                }
            }
        }
        self.goodbye().await;
    }

    /// Вежливое закрытие: DTLS close_notify и остановка SCTP дойдут до браузера, и он узнает об этом сразу.
    async fn goodbye(&mut self) {
        if self.rtc.close().is_err() {
            return;
        }
        for _ in 0..10 {
            let _ = self.rtc.handle_input(Input::Timeout(Instant::now()));
            self.drain();
            if !self.rtc.is_alive() {
                return;
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    fn finished(&self) -> bool {
        self.done
            || !self.rtc.is_alive()
            || self.room.cancel.is_cancelled()
            || self.room.expired()
            || (self.chan.is_none() && Instant::now() >= self.open_by)
    }

    /// Пакет из сети — в str0m. Чужие пакеты (сканеры, остатки старых сеансов) отбрасываем.
    fn input(&mut self, (from, local, data): Packet) {
        let Ok(recv) = Receive::new(Protocol::Udp, from, local, &data) else { return };
        let input = Input::Receive(Instant::now(), recv);
        if self.rtc.accepts(&input) && self.rtc.handle_input(input).is_err() {
            self.done = true;
        }
    }

    /// Забрать всё, что накопил str0m: пакеты уходят в сеть, события разбираются. Возвращает время следующего таймера.
    fn drain(&mut self) -> Instant {
        loop {
            match self.rtc.poll_output() {
                Ok(Output::Timeout(t)) => return t,
                Ok(Output::Transmit(t)) => self.transmit(t),
                Ok(Output::Event(e)) => self.on_event(e),
                Err(_) => {
                    self.done = true;
                    return Instant::now() + Duration::from_secs(1);
                }
            }
        }
    }

    fn transmit(&self, t: Transmit) {
        if let Some((_, sock)) = self.socks.iter().find(|(addr, _)| *addr == t.source) {
            // Потерянный UDP-пакет — обычное дело, SCTP повторит.
            let _ = sock.try_send_to(&t.contents, t.destination);
        }
    }

    fn on_event(&mut self, e: Event) {
        match e {
            Event::ChannelOpen(id, label) if label == "files" && self.chan.is_none() => {
                self.chan = Some(id);
                if let Some(mut ch) = self.rtc.channel(id) {
                    ch.set_buffered_amount_low_threshold(LOW_WATER);
                }
                self.slot.connect();
                self.hello();
            }
            // Команды чужих каналов и двоичные сообщения игнорируем.
            Event::ChannelData(d) if Some(d.id) == self.chan && !d.binary && d.data.len() <= 4096 => {
                self.inbox.push_back(d.data);
            }
            Event::ChannelClose(id) if Some(id) == self.chan => self.done = true,
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) | Event::Closed => {
                self.done = true
            }
            // Место в буфере освободилось: цикл и так подкачает данные после этого вызова.
            Event::ChannelBufferedAmountLow(_) => {}
            _ => {}
        }
    }

    /// Текстовое сообщение в канал; `false` — канал закрыт или буфер полон.
    fn say(&mut self, msg: Value) -> bool {
        let Some(mut ch) = self.chan.and_then(|id| self.rtc.channel(id)) else { return false };
        ch.write(false, msg.to_string().as_bytes()).unwrap_or(false)
    }

    fn hello(&mut self) {
        let files: Vec<Value> = (self.room.files.iter().enumerate())
            .map(|(i, f)| json!({"i": i, "name": f.name, "size": f.size}))
            .collect();
        let name = lock(&self.room.device_name).clone();
        self.say(json!({"t": "hello", "v": 1, "name": name, "expires": self.room.expires, "files": files}));
    }

    fn error(&mut self, i: Option<u64>, code: &str) {
        self.say(json!({"t": "error", "i": i, "code": code}));
    }

    async fn command(&mut self, raw: &[u8]) {
        let Ok(c) = serde_json::from_slice::<Command>(raw) else { return };
        match c.t.as_str() {
            "get" => self.start_get(c.i, c.offset.unwrap_or(0)).await,
            "cancel" => self.send = None,
            // Chromium теряет сообщение, посланное в тот же миг, когда канал
            // открылся: страница, не дождавшись hello, просит его ещё раз.
            "ready" => self.hello(),
            _ => {}
        }
    }

    async fn start_get(&mut self, i: Option<u64>, offset: u64) {
        if self.send.is_some() {
            return; // по одному файлу за раз
        }
        let Some(idx) = i.and_then(|n| usize::try_from(n).ok()).filter(|n| *n < self.room.files.len()) else {
            return self.error(i, "not_found");
        };
        if self.room.expired() {
            return self.error(i, "expired");
        }
        match open_file(&self.room.files[idx].path, offset).await {
            Ok((reader, size)) => {
                self.send = Some(Transfer {
                    i: idx,
                    size,
                    offset,
                    left: size - offset,
                    reader,
                    stage: Stage::Header,
                    pending: None,
                });
            }
            Err(code) => self.error(i, code),
        }
    }

    /// Пишем в канал, пока в буфере меньше `HIGH_WATER`. `true` — упёрлись в лимит хода, а не в буфер.
    async fn pump(&mut self) -> bool {
        for _ in 0..HIGH_WATER / CHUNK {
            if !self.step().await {
                return false;
            }
        }
        true
    }

    /// Один шаг отдачи: заголовок `file`, кусок данных или `end`. `false` — дальше идти нельзя.
    async fn step(&mut self) -> bool {
        let Some(id) = self.chan else { return false };
        if self.send.is_none() {
            return false;
        }
        match self.rtc.channel(id).map(|mut c| c.buffered_amount()) {
            None => {
                self.done = true;
                return false;
            }
            Some(n) if n >= HIGH_WATER => return false,
            Some(_) => {}
        }
        // Кусок читаем с диска до обращения к каналу.
        if let Some(t) =
            self.send.as_mut().filter(|t| t.stage == Stage::Data && t.pending.is_none() && t.left > 0)
        {
            let mut buf = vec![0u8; t.left.min(CHUNK as u64) as usize];
            if t.reader.read_exact(&mut buf).await.is_err() {
                let i = t.i as u64;
                self.send = None;
                self.error(Some(i), "read");
                return false;
            }
            t.pending = Some(buf);
        }
        let (Some(t), Some(mut ch)) = (self.send.as_mut(), self.rtc.channel(id)) else { return false };
        let written = match t.stage {
            Stage::Header => {
                let h = json!({"t": "file", "i": t.i, "size": t.size, "offset": t.offset});
                ch.write(false, h.to_string().as_bytes())
            }
            Stage::Data if t.left == 0 => {
                t.stage = Stage::End;
                return true;
            }
            Stage::Data => match t.pending.as_deref() {
                Some(chunk) => ch.write(true, chunk),
                None => return false,
            },
            Stage::End => ch.write(false, json!({"t": "end", "i": t.i}).to_string().as_bytes()),
        };
        match written {
            Ok(true) => {}
            Ok(false) => return false,
            Err(_) => {
                self.done = true;
                return false;
            }
        }
        match t.stage {
            Stage::Header => t.stage = Stage::Data,
            Stage::Data => t.left -= t.pending.take().map_or(0, |c| c.len() as u64),
            Stage::End => {
                // Скачиванием считаем целый файл с первого байта.
                let (i, full) = (t.i, t.offset == 0);
                self.send = None;
                if full {
                    let name = self.room.files[i].name.clone();
                    self.room.download_done(&name);
                    self.room.log(format!("файл {i} отдан целиком"));
                }
            }
        }
        true
    }
}

/// Предложение должно быть только про канал данных, без звука и видео (SRTP здесь не нужен).
/// Из кандидатов оставляем UDP с числовым адресом: браузеры прячут адрес локальной сети
/// за именем `*.local` (mDNS), его нам не разрешить, и TCP-кандидаты тоже не нужны.
fn data_only_sdp(sdp: &str) -> Option<String> {
    let mut out = String::with_capacity(sdp.len());
    for line in sdp.lines() {
        if line.starts_with("m=") && !line.starts_with("m=application") {
            return None;
        }
        if let Some(c) = line.strip_prefix("a=candidate:") {
            let f: Vec<&str> = c.split_whitespace().collect();
            if f.len() < 6 || !f[2].eq_ignore_ascii_case("udp") || f[4].parse::<IpAddr>().is_err() {
                continue;
            }
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    Some(out)
}

/// Открыть файл и встать на `offset`. Ошибка — код для браузера.
async fn open_file(path: &Path, offset: u64) -> Result<(BufReader<File>, u64), &'static str> {
    let mut f = File::open(path)
        .await
        .map_err(|e| if e.kind() == io::ErrorKind::NotFound { "not_found" } else { "read" })?;
    let size = f.metadata().await.map_err(|_| "read")?.len();
    if offset > size {
        return Err("read");
    }
    if offset > 0 {
        f.seek(SeekFrom::Start(offset)).await.map_err(|_| "read")?;
    }
    Ok((BufReader::with_capacity(256 * 1024, f), size))
}

/// По UDP-сокету на каждый адрес (без повторов, не больше `MAX_SOCKETS`).
async fn bind_sockets(ips: Vec<IpAddr>) -> Vec<(SocketAddr, Arc<UdpSocket>)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for ip in ips.into_iter().filter(|ip| seen.insert(*ip)).take(MAX_SOCKETS) {
        if let Ok(s) = UdpSocket::bind((ip, 0)).await
            && let Ok(addr) = s.local_addr()
        {
            out.push((addr, Arc::new(s)));
        }
    }
    out
}

/// Внешние адреса для сокетов IPv4: пары «адрес сокета, адрес снаружи NAT».
async fn reflexive(socks: &[(SocketAddr, Arc<UdpSocket>)], stun: &[String]) -> Vec<(SocketAddr, SocketAddr)> {
    if stun.is_empty() || !socks.iter().any(|(a, _)| a.is_ipv4()) {
        return Vec::new();
    }
    let servers = Arc::new(resolve_stun(stun).await);
    let mut set = JoinSet::new();
    for (local, sock) in socks.iter().filter(|(a, _)| a.is_ipv4()) {
        let (local, sock, servers) = (*local, sock.clone(), servers.clone());
        set.spawn(async move {
            let mapped = stun_binding(&sock, &servers).await?;
            (mapped != local).then_some((local, mapped))
        });
    }
    let mut out = Vec::new();
    while let Some(r) = set.join_next().await {
        out.extend(r.ok().flatten());
    }
    out
}

/// Читатель сокета: пакеты уходят в общую очередь получателя.
async fn read_socket(local: SocketAddr, sock: Arc<UdpSocket>, tx: mpsc::Sender<Packet>) {
    let mut buf = [0u8; 2048];
    loop {
        match sock.recv_from(&mut buf).await {
            Ok((n, from)) => {
                if tx.send((from, local, buf[..n].to_vec())).await.is_err() {
                    return;
                }
            }
            // Например, ICMP «порт недоступен» на Windows; не крутимся впустую.
            Err(_) => sleep(Duration::from_millis(10)).await,
        }
    }
}

// ---------------------------------------------------------------------------
// Криптография WebRTC без aws-lc
// ---------------------------------------------------------------------------

/// Свой провайдер для str0m. Готовый `rust-crypto` через dimpl включает генерацию
/// сертификатов на rcgen с aws-lc, а aws-lc-sys нам не нужен: он тяжёлый и плохо
/// собирается под мобильные платформы. Здесь то же самое, что в `str0m-rust-crypto`
/// (DTLS — dimpl на RustCrypto, HMAC-SHA1 для STUN, SHA-256 для отпечатков),
/// но сертификат делает rcgen поверх ring, который и так нужен iroh. SRTP не
/// используется (только DataChannel), шифры-заглушки возвращают ошибку.
mod crypto {
    use std::sync::Arc;
    use std::time::Instant;

    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    use str0m::crypto::dtls::{
        DtlsCert, DtlsImplError, DtlsInstance, DtlsOutput, DtlsProvider, DtlsVersion, ProtocolVersion,
    };
    use str0m::crypto::{
        AeadAes128GcmCipher, AeadAes256GcmCipher, Aes128CmSha1_80Cipher, CryptoError, CryptoProvider,
        Sha1HmacProvider, Sha256Provider, SrtpProvider, SupportedAeadAes128Gcm, SupportedAeadAes256Gcm,
        SupportedAes128CmSha1_80,
    };

    pub fn provider() -> Arc<CryptoProvider> {
        Arc::new(CryptoProvider {
            srtp_provider: &Stub,
            sha1_hmac_provider: &Stub,
            sha256_provider: &Stub,
            dtls_provider: &Stub,
        })
    }

    /// Самоподписанный сертификат ECDSA P-256 для DTLS: пара из DER сертификата и ключа PKCS#8.
    pub fn certificate() -> Option<DtlsCert> {
        use rcgen::{CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, PKCS_ECDSA_P256_SHA256};
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).ok()?;
        let mut params = CertificateParams::new(Vec::<String>::new()).ok()?;
        params.distinguished_name = DistinguishedName::new();
        params.distinguished_name.push(DnType::CommonName, "DTLS Peer");
        params.is_ca = IsCa::NoCa;
        // Браузер сверяет только отпечаток из SDP; даты берём с запасом, чтобы не зависеть от часов.
        params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        params.not_after = rcgen::date_time_ymd(2100, 1, 1);
        // Firefox требует различные серийные номера у разных сертификатов.
        params.serial_number = Some(rand::random::<[u8; 16]>().to_vec().into());
        let cert = params.self_signed(&key).ok()?;
        Some(DtlsCert { certificate: cert.der().to_vec(), private_key: key.serialize_der() })
    }

    /// Все части провайдера — одна пустая структура.
    #[derive(Debug)]
    struct Stub;

    impl Sha1HmacProvider for Stub {
        fn sha1_hmac(&self, key: &[u8], payloads: &[&[u8]]) -> [u8; 20] {
            // HMAC принимает ключ любой длины.
            let Ok(mut mac) = Hmac::<sha1::Sha1>::new_from_slice(key) else { return [0; 20] };
            payloads.iter().for_each(|p| mac.update(p));
            mac.finalize().into_bytes().into()
        }
    }

    impl Sha256Provider for Stub {
        fn sha256(&self, data: &[u8]) -> [u8; 32] {
            Sha256::digest(data).into()
        }
    }

    impl DtlsProvider for Stub {
        fn generate_certificate(&self) -> Option<DtlsCert> {
            certificate()
        }

        fn new_dtls(
            &self,
            cert: &DtlsCert,
            now: Instant,
            version: DtlsVersion,
            mtu: Option<usize>,
        ) -> Result<Box<dyn DtlsInstance>, CryptoError> {
            let cert = dimpl::DtlsCertificate {
                certificate: cert.certificate.clone(),
                private_key: cert.private_key.clone(),
            };
            // ICE проверяет обратную маршрутизацию до DTLS, cookie сервера не нужны.
            let mut builder = dimpl::Config::builder().use_server_cookie(false);
            if let Some(mtu) = mtu {
                builder = builder.mtu(mtu);
            }
            let cfg = Arc::new(builder.build().map_err(|e| CryptoError::Other(format!("dimpl: {e}")))?);
            let dtls = match version {
                DtlsVersion::Dtls12 => dimpl::Dtls::new_12(cfg, cert, now),
                DtlsVersion::Dtls13 => dimpl::Dtls::new_13(cfg, cert, now),
                DtlsVersion::Auto => dimpl::Dtls::new_auto(cfg, cert, now),
                v => return Err(CryptoError::Other(format!("версия DTLS не поддерживается: {v}"))),
            };
            Ok(Box::new(Session(dtls)))
        }
    }

    struct Session(dimpl::Dtls);

    impl std::fmt::Debug for Session {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Session")
        }
    }

    impl DtlsInstance for Session {
        fn set_active(&mut self, active: bool) {
            self.0.set_active(active);
        }
        fn handle_packet(&mut self, packet: &[u8]) -> Result<(), DtlsImplError> {
            self.0.handle_packet(packet)
        }
        fn poll_output<'a>(&mut self, buf: &'a mut [u8]) -> DtlsOutput<'a> {
            self.0.poll_output(buf)
        }
        fn handle_timeout(&mut self, now: Instant) -> Result<(), DtlsImplError> {
            self.0.handle_timeout(now)
        }
        fn send_application_data(&mut self, data: &[u8]) -> Result<(), DtlsImplError> {
            self.0.send_application_data(data)
        }
        fn is_active(&self) -> bool {
            self.0.is_active()
        }
        fn protocol_version(&self) -> Option<ProtocolVersion> {
            self.0.protocol_version()
        }
        fn is_closing(&self) -> bool {
            self.0.is_closing()
        }
        fn is_closed(&self) -> bool {
            self.0.is_closed()
        }
        fn close(&mut self) -> Result<(), DtlsImplError> {
            self.0.close()
        }
    }

    fn no_srtp() -> CryptoError {
        CryptoError::Other("SRTP не поддерживается".to_string())
    }

    impl SrtpProvider for Stub {
        fn aes_128_cm_sha1_80(&self) -> &'static dyn SupportedAes128CmSha1_80 {
            &Stub
        }
        fn aead_aes_128_gcm(&self) -> &'static dyn SupportedAeadAes128Gcm {
            &Stub
        }
        fn aead_aes_256_gcm(&self) -> &'static dyn SupportedAeadAes256Gcm {
            &Stub
        }
        fn srtp_aes_128_ecb_round(&self, _key: &[u8], _input: &[u8], output: &mut [u8]) {
            output.fill(0);
        }
        fn srtp_aes_256_ecb_round(&self, _key: &[u8], _input: &[u8], output: &mut [u8]) {
            output.fill(0);
        }
    }

    impl SupportedAes128CmSha1_80 for Stub {
        fn create_cipher(&self, _key: [u8; 16], _encrypt: bool) -> Box<dyn Aes128CmSha1_80Cipher> {
            Box::new(Stub)
        }
    }

    impl SupportedAeadAes128Gcm for Stub {
        fn create_cipher(&self, _key: [u8; 16], _encrypt: bool) -> Box<dyn AeadAes128GcmCipher> {
            Box::new(Stub)
        }
    }

    impl SupportedAeadAes256Gcm for Stub {
        fn create_cipher(&self, _key: [u8; 32], _encrypt: bool) -> Box<dyn AeadAes256GcmCipher> {
            Box::new(Stub)
        }
    }

    impl Aes128CmSha1_80Cipher for Stub {
        fn encrypt(&mut self, _iv: &[u8; 16], _input: &[u8], _output: &mut [u8]) -> Result<(), CryptoError> {
            Err(no_srtp())
        }
        fn decrypt(&mut self, _iv: &[u8; 16], _input: &[u8], _output: &mut [u8]) -> Result<(), CryptoError> {
            Err(no_srtp())
        }
    }

    impl AeadAes128GcmCipher for Stub {
        fn encrypt(
            &mut self,
            _iv: &[u8; 12],
            _aad: &[u8],
            _input: &[u8],
            _output: &mut [u8],
        ) -> Result<(), CryptoError> {
            Err(no_srtp())
        }
        fn decrypt(
            &mut self,
            _iv: &[u8; 12],
            _aads: &[&[u8]],
            _input: &[u8],
            _output: &mut [u8],
        ) -> Result<usize, CryptoError> {
            Err(no_srtp())
        }
    }

    impl AeadAes256GcmCipher for Stub {
        fn encrypt(
            &mut self,
            _iv: &[u8; 12],
            _aad: &[u8],
            _input: &[u8],
            _output: &mut [u8],
        ) -> Result<(), CryptoError> {
            Err(no_srtp())
        }
        fn decrypt(
            &mut self,
            _iv: &[u8; 12],
            _aads: &[&[u8]],
            _input: &[u8],
            _output: &mut [u8],
        ) -> Result<usize, CryptoError> {
            Err(no_srtp())
        }
    }
}

// ---------------------------------------------------------------------------
// Тесты
// ---------------------------------------------------------------------------

/// Почтовый ящик службы связи в памяти: те же правила, что у настоящей, включая длинный запрос.
#[cfg(test)]
mod mem {
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;

    use tokio::sync::Notify;

    use super::*;

    #[derive(Default)]
    pub struct MemBox {
        state: Mutex<State>,
        wake: Notify,
        /// «Службы нет»: все вызовы заканчиваются сетевой ошибкой.
        pub down: AtomicBool,
    }

    #[derive(Default)]
    struct State {
        rooms: HashMap<String, MemRoom>,
    }

    struct MemRoom {
        owner: String,
        offers: Vec<(u64, String, String)>,
        answers: HashMap<String, String>,
    }

    impl MemBox {
        fn up(&self) -> Result<(), SigError> {
            if self.down.load(Ordering::SeqCst) { Err(SigError::Net) } else { Ok(()) }
        }

        pub fn rooms(&self) -> usize {
            lock(&self.state).rooms.len()
        }

        pub fn put_room(&self, room: &str, owner: &str, ttl: u64) -> Result<(), SigError> {
            self.up()?;
            if !ROOM_TTL.contains(&ttl) {
                return Err(SigError::Status(400));
            }
            let mut st = lock(&self.state);
            let r = st.rooms.entry(room.to_string()).or_insert_with(|| MemRoom {
                owner: owner.to_string(),
                offers: Vec::new(),
                answers: HashMap::new(),
            });
            if r.owner == owner { Ok(()) } else { Err(SigError::Status(409)) }
        }

        pub fn delete_room(&self, room: &str, owner: &str) -> Result<(), SigError> {
            self.up()?;
            let mut st = lock(&self.state);
            match st.rooms.get(room) {
                None => Err(SigError::Status(404)),
                Some(r) if r.owner != owner => Err(SigError::Status(403)),
                Some(_) => {
                    st.rooms.remove(room);
                    drop(st);
                    self.wake.notify_waiters();
                    Ok(())
                }
            }
        }

        /// Браузер: положить предложение, получить `sid`.
        pub fn post_offer(&self, room: &str, msg: &str) -> Result<String, SigError> {
            self.up()?;
            let mut st = lock(&self.state);
            let r = st.rooms.get_mut(room).ok_or(SigError::Status(404))?;
            if r.offers.len() >= 64 {
                return Err(SigError::Status(429));
            }
            let sid = B64.encode(&rand::random::<[u8; 9]>());
            r.offers.push((r.offers.len() as u64 + 1, sid.clone(), msg.to_string()));
            drop(st);
            self.wake.notify_waiters();
            Ok(sid)
        }

        pub async fn offers(
            &self,
            room: &str,
            owner: &str,
            after: u64,
            wait: u64,
        ) -> Result<Vec<Offer>, SigError> {
            let end = tokio::time::Instant::now() + Duration::from_secs(wait);
            loop {
                // Подписка до проверки: иначе можно пропустить предложение, пришедшее между ними.
                let woken = self.wake.notified();
                tokio::pin!(woken);
                woken.as_mut().enable();
                self.up()?;
                {
                    let st = lock(&self.state);
                    let r = st.rooms.get(room).ok_or(SigError::Status(404))?;
                    if r.owner != owner {
                        return Err(SigError::Status(403));
                    }
                    let list: Vec<Offer> = (r.offers.iter().filter(|o| o.0 > after))
                        .map(|o| Offer { seq: o.0, sid: o.1.clone(), msg: o.2.clone() })
                        .collect();
                    if !list.is_empty() {
                        return Ok(list);
                    }
                }
                if timeout_at(end, woken).await.is_err() {
                    return Ok(Vec::new());
                }
            }
        }

        pub fn put_answer(&self, room: &str, owner: &str, sid: &str, msg: &str) -> Result<(), SigError> {
            self.up()?;
            let mut st = lock(&self.state);
            let r = st.rooms.get_mut(room).ok_or(SigError::Status(404))?;
            if r.owner != owner {
                return Err(SigError::Status(403));
            }
            r.answers.insert(sid.to_string(), msg.to_string());
            drop(st);
            self.wake.notify_waiters();
            Ok(())
        }

        /// Браузер: ждать ответа; `None` — не дождался.
        pub async fn answer(&self, room: &str, sid: &str, wait: u64) -> Result<Option<String>, SigError> {
            let end = tokio::time::Instant::now() + Duration::from_secs(wait);
            loop {
                let woken = self.wake.notified();
                tokio::pin!(woken);
                woken.as_mut().enable();
                self.up()?;
                {
                    let st = lock(&self.state);
                    let r = st.rooms.get(room).ok_or(SigError::Status(404))?;
                    if let Some(m) = r.answers.get(sid) {
                        return Ok(Some(m.clone()));
                    }
                }
                if timeout_at(end, woken).await.is_err() {
                    return Ok(None);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use str0m::change::SdpAnswer;

    use super::mem::MemBox;
    use super::*;
    use crate::util::random_id;

    fn cipher() -> Aes256Gcm {
        Aes256Gcm::new(&Key::<Aes256Gcm>::from(rand::random::<[u8; 32]>()))
    }

    #[test]
    fn message_crypto() {
        let c = cipher();
        let aad = "ogshare-v1|offer|room";
        let msg = seal(&c, aad, br#"{"sdp":"x"}"#).unwrap();
        // base64url без «=», «+» и «/»; длина — nonce 12 + текст + тег 16.
        assert!(msg.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
        assert_eq!(B64.decode(msg.as_bytes()).unwrap().len(), 12 + 11 + 16);
        assert_eq!(open(&c, aad, &msg).unwrap(), br#"{"sdp":"x"}"#);
        // Чужой AAD (другая комната, ответ вместо предложения), чужой ключ, порча, мусор.
        assert!(open(&c, "ogshare-v1|offer|other", &msg).is_none());
        assert!(open(&c, "ogshare-v1|answer|room|sid", &msg).is_none());
        assert!(open(&cipher(), aad, &msg).is_none());
        let mut bad = B64.decode(msg.as_bytes()).unwrap();
        bad[14] ^= 1;
        assert!(open(&c, aad, &B64.encode(&bad)).is_none());
        assert!(open(&c, aad, "!!!").is_none());
        assert!(open(&c, aad, "AAAA").is_none());
        // Каждый раз новый nonce.
        assert_ne!(seal(&c, aad, b"a"), seal(&c, aad, b"a"));
    }

    #[test]
    fn stun_known_answer() {
        // Ответ из RFC 5769 (2.1): SOFTWARE «test vector» (с выравниванием) и XOR-MAPPED-ADDRESS 192.0.2.1:32853.
        let txn: [u8; 12] = [0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae];
        let mut m = vec![0x01, 0x01, 0x00, 0x1c, 0x21, 0x12, 0xa4, 0x42];
        m.extend(txn);
        m.extend([0x80, 0x22, 0x00, 0x0b]);
        m.extend(b"test vector\0");
        m.extend([0x00, 0x20, 0x00, 0x08, 0x00, 0x01, 0xa1, 0x47, 0xe1, 0x12, 0xa6, 0x43]);
        assert_eq!(parse_stun_response(&m, &txn), Some("192.0.2.1:32853".parse().unwrap()));
        // Чужой transaction id, обрезанный и пустой пакет.
        assert_eq!(parse_stun_response(&m, &[0; 12]), None);
        assert_eq!(parse_stun_response(&m[..30], &txn), None);
        assert_eq!(parse_stun_response(&[], &txn), None);
        // Запрос: 20 байт, magic cookie, наш transaction id.
        let req = stun_request(&txn);
        assert_eq!(
            (req[0..2].to_vec(), req[4..8].to_vec(), req[8..].to_vec()),
            (vec![0, 1], STUN_COOKIE.to_vec(), txn.to_vec())
        );
    }

    #[tokio::test]
    async fn stun_against_fake_server() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 64];
            while let Ok((n, from)) = server.recv_from(&mut buf).await {
                if n != 20 || buf[4..8] != STUN_COOKIE {
                    continue;
                }
                // Binding Success с XOR-MAPPED-ADDRESS = адрес отправителя.
                let SocketAddr::V4(v4) = from else { continue };
                let mut r = vec![0x01, 0x01, 0x00, 0x0c];
                r.extend(&buf[4..20]);
                r.extend([0x00, 0x20, 0x00, 0x08, 0x00, 0x01]);
                r.extend((v4.port() ^ 0x2112).to_be_bytes());
                r.extend(v4.ip().octets().iter().zip(STUN_COOKIE).map(|(a, c)| a ^ c));
                let _ = server.send_to(&r, from).await;
            }
        });
        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mapped = stun_binding(&client, &[server_addr]).await;
        assert_eq!(mapped, Some(client.local_addr().unwrap()));
        // Сервер, который молчит: ответа нет, но и зависания нет.
        let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let t = Instant::now();
        assert_eq!(stun_binding(&client, &[silent.local_addr().unwrap()]).await, None);
        assert!(t.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn offer_is_filtered() {
        let sdp = "v=0\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n\
            a=candidate:1 1 udp 2113937151 7f1a2b3c-4d5e.local 54322 typ host\r\n\
            a=candidate:2 1 tcp 1518280447 192.0.2.7 9 typ host tcptype active\r\n\
            a=candidate:3 1 UDP 1677729535 203.0.113.5 54321 typ srflx raddr 0.0.0.0 rport 0\r\n\
            a=candidate:4 1 udp 2113937151 2001:db8::1 40000 typ host\r\na=mid:0\r\n";
        let out = data_only_sdp(sdp).unwrap();
        assert_eq!(out.matches("a=candidate:").count(), 2);
        assert!(out.contains("203.0.113.5") && out.contains("2001:db8::1") && out.contains("a=mid:0"));
        assert!(!out.contains(".local") && !out.contains("tcp"));
        // Звук или видео — не наш случай.
        assert!(data_only_sdp("v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\n").is_none());
    }

    // ----- «браузер»: вторая str0m в роли предлагающей стороны -----

    enum Msg {
        Text(Value),
        Bin(Vec<u8>),
    }

    struct Browser {
        rtc: Rtc,
        sock: UdpSocket,
        cid: ChannelId,
        inbox: VecDeque<Msg>,
        closed: bool,
    }

    impl Browser {
        /// Предложение с каналом «files» и адресом 127.0.0.1; `extra` — дополнительные строки кандидатов.
        async fn offer(extra: &str) -> (Browser, String, str0m::change::SdpPendingOffer) {
            let sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let mut rtc = Rtc::builder().set_crypto_provider(crypto::provider()).build(Instant::now());
            rtc.add_local_candidate(Candidate::host(sock.local_addr().unwrap(), "udp").unwrap());
            let mut api = rtc.sdp_api();
            let cid = api.add_channel("files".into());
            let (offer, pending) = api.apply().unwrap();
            let sdp = offer.to_sdp_string().replacen("a=ice-ufrag", &format!("{extra}a=ice-ufrag"), 1);
            let b = Browser { rtc, sock, cid, inbox: VecDeque::new(), closed: false };
            (b, sdp, pending)
        }

        async fn step(&mut self) {
            let next = loop {
                match self.rtc.poll_output().unwrap() {
                    Output::Timeout(t) => break t,
                    Output::Transmit(t) => {
                        let _ = self.sock.send_to(&t.contents, t.destination).await;
                    }
                    Output::Event(Event::ChannelData(d)) => self.inbox.push_back(if d.binary {
                        Msg::Bin(d.data)
                    } else {
                        Msg::Text(serde_json::from_slice(&d.data).unwrap())
                    }),
                    Output::Event(Event::ChannelClose(_) | Event::Closed) => self.closed = true,
                    Output::Event(_) => {}
                }
            };
            if self.closed {
                return;
            }
            let mut buf = [0u8; 2048];
            tokio::select! {
                r = self.sock.recv_from(&mut buf) => {
                    if let Ok((n, from)) = r {
                        let local = self.sock.local_addr().unwrap();
                        if let Ok(recv) = Receive::new(Protocol::Udp, from, local, &buf[..n]) {
                            self.rtc.handle_input(Input::Receive(Instant::now(), recv)).unwrap();
                        }
                    }
                }
                _ = sleep_until(at(next)) => self.rtc.handle_input(Input::Timeout(Instant::now())).unwrap(),
            }
        }

        async fn next(&mut self) -> Msg {
            loop {
                if let Some(m) = self.inbox.pop_front() {
                    return m;
                }
                self.step().await;
            }
        }

        async fn next_text(&mut self) -> Value {
            loop {
                if let Msg::Text(v) = self.next().await {
                    return v;
                }
            }
        }

        async fn say(&mut self, v: Value) {
            let mut ch = self.rtc.channel(self.cid).expect("канал открыт");
            assert!(ch.write(false, v.to_string().as_bytes()).unwrap());
            self.step().await;
        }

        /// `get` и всё, что за ним: заголовок, куски, `end`.
        async fn fetch(&mut self, i: u64, offset: u64, size: u64) -> Vec<u8> {
            self.say(json!({"t": "get", "i": i, "offset": offset})).await;
            let head = self.next_text().await;
            assert_eq!(head, json!({"t": "file", "i": i, "size": size, "offset": offset}));
            let mut data = Vec::new();
            loop {
                match self.next().await {
                    Msg::Bin(c) => {
                        assert!(c.len() <= CHUNK);
                        data.extend(c);
                    }
                    Msg::Text(v) => {
                        assert_eq!(v, json!({"t": "end", "i": i}));
                        return data;
                    }
                }
            }
        }
    }

    fn pattern(len: usize, k: usize) -> Vec<u8> {
        (0..len).map(|i| ((i * k + i / 251) % 253) as u8).collect()
    }

    async fn until(what: &str, mut ok: impl FnMut() -> bool) {
        for _ in 0..500 {
            if ok() {
                return;
            }
            sleep(Duration::from_millis(20)).await;
        }
        panic!("не дождались: {what}");
    }

    fn make_room(files: Vec<WebFile>, mem: &Arc<MemBox>) -> (Arc<Room>, Arc<Events>) {
        let events = Arc::new(Events::default());
        let cfg =
            RtcConfig { api: "mem://".into(), page: "https://example.test/share/get/".into(), stun: vec![] };
        let ips: IpSource = Arc::new(|| vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]);
        let room = Room::new(
            cfg,
            "s1".into(),
            "Телефон".into(),
            files,
            Duration::from_secs(3 * 3600),
            ips,
            events.clone(),
            CancellationToken::new(),
        );
        room.spawn(Some(Backend::Memory(mem.clone())));
        (room, events)
    }

    #[tokio::test]
    async fn serves_files_to_a_browser() {
        let dir = std::env::temp_dir().join(format!("osh-rtc-{}", random_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (pattern(300_000, 7), pattern(2_000_000, 3));
        let mut files = Vec::new();
        for (disk, name, data) in [("a.bin", "Отпуск/a.bin", &a), ("b.bin", "b.bin", &b)] {
            std::fs::write(dir.join(disk), data).unwrap();
            files.push(WebFile { path: dir.join(disk), name: name.into(), size: data.len() as u64 });
        }
        files.push(WebFile { path: dir.join("нет.bin"), name: "нет.bin".into(), size: 5 });

        let mem = Arc::new(MemBox::default());
        let (room, events) = make_room(files, &mem);
        assert_eq!(room.status(), "connecting");
        until("комната создана", || room.status() == "ready").await;
        assert_eq!(mem.rooms(), 1);

        // Ссылка: страница#room.key, 16 и 32 байта в base64url.
        let (page, frag) = room.link().split_once('#').unwrap();
        let (room_id, key) = frag.split_once('.').unwrap();
        assert_eq!(page, "https://example.test/share/get/");
        assert_eq!((room_id.len(), key.len()), (22, 43));
        let key: [u8; 32] = B64.decode(key.as_bytes()).unwrap().try_into().unwrap();
        let c = Aes256Gcm::new(&Key::<Aes256Gcm>::from(key));

        let run = async {
            // Предложение браузера; в нём «чужие» кандидаты, которые приложение должно отбросить.
            let extra = "a=candidate:1 1 udp 2113937151 7f1a2b3c-4d5e-6f70.local 54322 typ host\r\n\
                a=candidate:2 1 tcp 1518280447 192.0.2.7 9 typ host tcptype active\r\n";
            let (mut br, sdp, pending) = Browser::offer(extra).await;
            let msg = seal(
                &c,
                &format!("ogshare-v1|offer|{room_id}"),
                json!({"sdp": sdp, "lang": "ru"}).to_string().as_bytes(),
            )
            .unwrap();
            // Чужое сообщение в комнате не мешает.
            mem.post_offer(room_id, "мусор").unwrap();
            let sid = mem.post_offer(room_id, &msg).unwrap();
            assert_eq!(sid.len(), 12);
            let reply = mem.answer(room_id, &sid, 20).await.unwrap().expect("ответ получен");
            assert!(open(&c, &format!("ogshare-v1|offer|{room_id}"), &reply).is_none());
            let plain =
                open(&c, &format!("ogshare-v1|answer|{room_id}|{sid}"), &reply).expect("ответ расшифрован");
            let sdp = serde_json::from_slice::<Value>(&plain).unwrap()["sdp"].as_str().unwrap().to_string();
            assert!(sdp.contains("a=candidate:") && sdp.contains("127.0.0.1"));
            br.rtc.sdp_api().accept_answer(pending, SdpAnswer::from_sdp_string(&sdp).unwrap()).unwrap();

            let hello = br.next_text().await;
            assert_eq!(hello["t"], "hello");
            assert_eq!(hello["v"], 1);
            assert_eq!(hello["name"], "Телефон");
            assert!(hello["expires"].as_u64().unwrap() > now_unix() + 3 * 3600 - 10);
            assert_eq!(hello["files"][0], json!({"i": 0, "name": "Отпуск/a.bin", "size": 300_000}));
            assert_eq!(hello["files"][1], json!({"i": 1, "name": "b.bin", "size": 2_000_000}));
            assert_eq!(room.peers(), 1);

            // Целиком — считается; с середины — нет.
            assert!(br.fetch(0, 0, 300_000).await == a);
            assert_eq!(room.downloads(), 1);
            assert!(br.fetch(1, 1_000_000, 2_000_000).await == b[1_000_000..]);
            assert_eq!(room.downloads(), 1);

            // Ошибки: нет номера, нет файла на диске, смещение за концом.
            for (i, offset, code) in [(9, 0, "not_found"), (2, 0, "not_found"), (0, 300_001, "read")] {
                br.say(json!({"t": "get", "i": i, "offset": offset})).await;
                assert_eq!(br.next_text().await, json!({"t": "error", "i": i, "code": code}));
            }

            // Отмена посреди файла: после неё можно просить другой, старые куски отбрасываем.
            br.say(json!({"t": "get", "i": 1, "offset": 1})).await;
            for _ in 0..3 {
                while !matches!(br.next().await, Msg::Bin(_)) {}
            }
            br.say(json!({"t": "cancel"})).await;
            br.say(json!({"t": "get", "i": 0, "offset": 299_990})).await;
            while br.next_text().await["t"] != "file" {}
            let mut tail = Vec::new();
            loop {
                match br.next().await {
                    Msg::Bin(c) => tail.extend(c),
                    Msg::Text(v) if v["t"] == "end" && v["i"] == 0 => break,
                    Msg::Text(_) => {}
                }
            }
            assert_eq!(tail, a[299_990..]);
            assert_eq!(room.downloads(), 1);
            br
        };
        let Ok(mut br) = timeout(Duration::from_secs(60), run).await else {
            panic!("сценарий не уложился во время; журнал: {:#?}", events.log_lines());
        };

        // Одно событие на скачивание, имя — как в списке.
        let events_json = events.poll();
        let downloads: Vec<&Value> = events_json["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "web_download")
            .collect();
        assert_eq!(downloads, [&json!({"type": "web_download", "share": "s1", "name": "Отпуск/a.bin"})]);

        // Остановка: получатель отключается, комната удаляется, браузер видит закрытие.
        room.close();
        until("получатель завершился", || {
            room.active.load(Ordering::SeqCst) == 0 && room.peers() == 0
        })
        .await;
        until("комната удалена", || mem.rooms() == 0).await;
        timeout(Duration::from_secs(5), async {
            while !br.closed {
                br.step().await;
            }
        })
        .await
        .expect("браузер увидел закрытие");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Службу связи по настоящему HTTPS: клиент, TLS (rustls + ring), заголовки, тела и коды ответов.
    #[tokio::test]
    async fn http_backend_talks_tls() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio_rustls::TlsAcceptor;
        use tokio_rustls::rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
        use tokio_rustls::rustls::{ServerConfig, crypto::ring};

        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        params.subject_alt_names = vec![rcgen::SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
        let cert = params.self_signed(&key).unwrap();
        let server_cfg = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )
            .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(server_cfg));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let (acceptor, log) = (acceptor.clone(), log.clone());
                tokio::spawn(async move {
                    let mut tls = acceptor.accept(tcp).await.unwrap();
                    let mut req = Vec::new();
                    let mut buf = [0u8; 2048];
                    // Заголовок и тело (длина — из Content-Length).
                    let (head_len, body_len) = loop {
                        let n = tls.read(&mut buf).await.unwrap();
                        req.extend(&buf[..n]);
                        let text = String::from_utf8_lossy(&req).into_owned();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let len = text.to_lowercase().split("content-length: ").nth(1).map_or(0, |v| {
                                v.split("\r\n").next().unwrap().trim().parse::<usize>().unwrap()
                            });
                            if req.len() >= end + 4 + len {
                                break (end + 4, len);
                            }
                        }
                    };
                    let text = String::from_utf8_lossy(&req).into_owned();
                    let line = text.lines().next().unwrap().to_string();
                    let owner =
                        (text.lines().find_map(|l| l.strip_prefix("x-owner: "))).unwrap_or("-").to_string();
                    let body = String::from_utf8_lossy(&req[head_len..head_len + body_len]).into_owned();
                    log.lock().unwrap().push(format!("{line} owner={owner} body={body}"));
                    let (status, reply) = match line.split(' ').take(2).collect::<Vec<_>>()[..] {
                        ["PUT", "/api/rooms/busy"] => ("409 Conflict", ""),
                        ["PUT", "/api/rooms/r1"] => ("200 OK", r#"{"expires":1}"#),
                        ["GET", "/api/rooms/r1/offers?after=7&wait=1"] => {
                            ("200 OK", r#"{"offers":[{"seq":8,"sid":"abc","msg":"zz"}]}"#)
                        }
                        ["PUT", "/api/rooms/r1/answers/abc"] | ["DELETE", "/api/rooms/r1"] => {
                            ("204 No Content", "")
                        }
                        _ => ("404 Not Found", ""),
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                        reply.len()
                    );
                    tls.write_all(resp.as_bytes()).await.unwrap();
                    tls.shutdown().await.ok();
                });
            }
        });

        let roots = iroh::tls::CaTlsConfig::custom_roots([cert.der().clone()]);
        let b = Backend::http(&format!("https://127.0.0.1:{port}/api/"), roots).unwrap();
        b.put_room("r1", "own", 120).await.unwrap();
        assert_eq!(b.put_room("busy", "own", 120).await, Err(SigError::Status(409)));
        let offers = b.offers("r1", "own", 7, 1).await.unwrap();
        assert_eq!((offers[0].seq, offers[0].sid.as_str(), offers[0].msg.as_str()), (8, "abc", "zz"));
        b.put_answer("r1", "own", "abc", "ответ").await.unwrap();
        b.delete_room("r1", "own").await.unwrap();
        assert_eq!(b.delete_room("zz", "own").await, Err(SigError::Status(404)));
        assert_eq!(
            *seen.lock().unwrap(),
            [
                r#"PUT /api/rooms/r1 HTTP/1.1 owner=own body={"ttl":120}"#,
                r#"PUT /api/rooms/busy HTTP/1.1 owner=own body={"ttl":120}"#,
                "GET /api/rooms/r1/offers?after=7&wait=1 HTTP/1.1 owner=own body=",
                r#"PUT /api/rooms/r1/answers/abc HTTP/1.1 owner=own body={"msg":"ответ"}"#,
                "DELETE /api/rooms/r1 HTTP/1.1 owner=own body=",
                "DELETE /api/rooms/zz HTTP/1.1 owner=own body=",
            ]
        );
        // Недоверенный сертификат — это сетевая ошибка, а не паника.
        let strict = Backend::http(&format!("https://127.0.0.1:{port}/api"), Default::default()).unwrap();
        assert_eq!(strict.put_room("r1", "own", 120).await, Err(SigError::Net));
    }

    #[tokio::test(start_paused = true)]
    async fn status_follows_the_service() {
        let mem = Arc::new(MemBox::default());
        mem.down.store(true, Ordering::SeqCst);
        let (room, _) = make_room(Vec::new(), &mem);
        assert_eq!(room.status(), "connecting");
        sleep(Duration::from_secs(1)).await;
        assert_eq!(room.status(), "offline");
        // Служба вернулась: через паузу повтора комната создаётся, статус «ready».
        mem.down.store(false, Ordering::SeqCst);
        sleep(RETRY + Duration::from_secs(1)).await;
        assert_eq!((room.status(), mem.rooms()), ("ready", 1));
        // Служба потеряла комнату (перезапуск): приложение создаёт её заново.
        mem.delete_room(&room.room, &room.owner).unwrap();
        sleep(Duration::from_secs(2)).await;
        assert_eq!((room.status(), mem.rooms()), ("ready", 1));
        room.close();
        sleep(Duration::from_secs(1)).await;
        assert_eq!(mem.rooms(), 0);
    }
}
