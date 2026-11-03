//! Служба связи Ogneliss Share: почтовый ящик для зашифрованных SDP.
//!
//! Устройство-отправитель открывает «комнату», браузер кладёт в неё
//! предложение WebRTC, устройство забирает его и кладёт ответ. Служба не
//! расшифровывает ничего: `msg` для неё непрозрачная строка. Состояние только
//! в памяти, на диск не пишется ничего. Протокол — `docs/WEB_P2P.md`, раздел 2.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tokio::time::{Instant, timeout_at};

/// Предел размера любого тела запроса (иначе 413).
pub const MAX_BODY: usize = 16 * 1024;
/// Допустимое время жизни комнаты после последнего `PUT`, секунды.
const MIN_TTL: u64 = 60;
const MAX_TTL: u64 = 3600;
/// Долгий опрос: ожидание по умолчанию и потолок, секунды.
const DEFAULT_WAIT: u64 = 25;
const MAX_WAIT: u64 = 30;
/// Сколько разных IP помним для ограничения частоты (защита памяти).
const MAX_TRACKED_IPS: usize = 100_000;
/// Длина секрета владельца в заголовке `X-Owner` (приложение шлёт 22 символа).
const OWNER_LEN: std::ops::RangeInclusive<usize> = 16..=128;
const ID_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";

/// Ограничения службы; в тестах подменяются.
#[derive(Clone, Debug)]
pub struct Config {
    /// Комнат одновременно; сверх — 503.
    pub max_rooms: usize,
    /// Предложений в одной комнате; сверх — 429.
    pub max_offers: usize,
    /// Запросов `PUT /rooms` и `POST …/offers` с одного IP за окно; сверх — 429.
    pub rate_limit: u32,
    pub rate_window: Duration,
    /// Общий предел хранимых сообщений в байтах; сверх — 503. Держит память
    /// в границах контейнера: 10 000 комнат × 64 × 16 КиБ иначе дали бы гигабайты.
    pub max_bytes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_rooms: 10_000,
            max_offers: 64,
            rate_limit: 120,
            rate_window: Duration::from_secs(60),
            max_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Serialize)]
struct Offer {
    seq: u64,
    sid: String,
    msg: String,
}

struct Room {
    /// SHA-256 от секрета владельца; сам секрет не хранится.
    owner: [u8; 32],
    expires: Instant,
    expires_unix: u64,
    seq: u64,
    offers: Vec<Offer>,
    /// Ответы по `sid`.
    answers: HashMap<String, String>,
    /// Сколько байт сообщений занимает комната (для общего предела).
    bytes: usize,
    /// «Звонок» для долгих опросов: любое изменение комнаты увеличивает версию,
    /// а удаление комнаты закрывает канал — ждущие просыпаются и перечитывают.
    bell: watch::Sender<()>,
}

impl Room {
    fn new(owner: [u8; 32], expires: Instant, expires_unix: u64) -> Room {
        Room {
            owner,
            expires,
            expires_unix,
            seq: 0,
            offers: Vec::new(),
            answers: HashMap::new(),
            bytes: 0,
            bell: watch::channel(()).0,
        }
    }

    fn ring(&self) {
        self.bell.send_replace(());
    }
}

#[derive(Default)]
struct Store {
    rooms: HashMap<String, Room>,
    /// Сумма `Room::bytes` по всем комнатам.
    bytes: usize,
}

impl Store {
    /// Комната, если она есть и не истекла. Истёкшая сразу считается
    /// отсутствующей (и убирается), не дожидаясь уборщика.
    fn live(&mut self, id: &str, now: Instant) -> Option<(&mut Room, &mut usize)> {
        if self.rooms.get(id).is_some_and(|r| r.expires <= now) {
            self.remove(id, "истекла");
        }
        let Store { rooms, bytes } = self;
        rooms.get_mut(id).map(|r| (r, bytes))
    }

    fn remove(&mut self, id: &str, why: &str) {
        if let Some(room) = self.rooms.remove(id) {
            self.bytes -= room.bytes;
            // Для журнала хватает начала имени; сообщений в журнале нет никогда.
            eprintln!("комната {}… {why}", &id[..id.len().min(6)]);
        }
    }

    fn purge(&mut self, now: Instant) {
        let dead: Vec<String> =
            self.rooms.iter().filter(|(_, r)| r.expires <= now).map(|(id, _)| id.clone()).collect();
        for id in dead {
            self.remove(&id, "истекла");
        }
    }
}

/// Общее состояние службы.
pub struct AppState {
    cfg: Config,
    store: Mutex<Store>,
    /// IP (для IPv6 — сеть /64) → (начало окна, запросов в окне).
    limiter: Mutex<HashMap<IpAddr, (Instant, u32)>>,
    /// `true` при остановке: долгие опросы отвечают сразу, не задерживая выход.
    closing: watch::Sender<bool>,
}

impl AppState {
    pub fn new(cfg: Config) -> Arc<AppState> {
        Arc::new(AppState {
            cfg,
            store: Mutex::new(Store::default()),
            limiter: Mutex::new(HashMap::new()),
            closing: watch::channel(false).0,
        })
    }

    /// Уборка: убирает истёкшие комнаты и устаревшие счётчики частоты.
    /// Вызывается раз в 30 секунд.
    pub fn sweep(&self) {
        let now = Instant::now();
        lock(&self.store).purge(now);
        let window = self.cfg.rate_window;
        lock(&self.limiter).retain(|_, (start, _)| now.duration_since(*start) < window);
    }

    /// Число комнат в памяти (вместе с истёкшими, которых ещё не убрали).
    pub fn rooms(&self) -> usize {
        lock(&self.store).rooms.len()
    }

    /// Остановка: ждущие опросы получают пустой ответ.
    pub fn close(&self) {
        self.closing.send_replace(true);
    }

    /// Учёт запроса IP; `false` — лимит исчерпан.
    fn allow(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let window = self.cfg.rate_window;
        let mut map = lock(&self.limiter);
        if map.len() >= MAX_TRACKED_IPS {
            map.retain(|_, (start, _)| now.duration_since(*start) < window);
            if map.len() >= MAX_TRACKED_IPS {
                map.clear();
            }
        }
        let entry = map.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        entry.1 += 1;
        entry.1 <= self.cfg.rate_limit
    }

    /// Долгий опрос комнаты. `check` смотрит на комнату: `Err` — сразу ответ
    /// с ошибкой, `Ok(Some)` — готово, `Ok(None)` — ждём изменений. Не дождались
    /// за `wait` — `Ok(None)`; комната пропала или истекла — 404.
    async fn poll<T>(
        &self,
        room: &str,
        wait: Duration,
        mut check: impl FnMut(&Room) -> Result<Option<T>, ApiError>,
    ) -> Result<Option<T>, ApiError> {
        let deadline = Instant::now() + wait;
        let mut closing = self.closing.subscribe();
        loop {
            // Подписка оформляется под замком, поэтому изменение между проверкой
            // и ожиданием не потеряется: `changed()` сработает сразу.
            let (mut bell, until) = {
                let mut store = lock(&self.store);
                let now = Instant::now();
                let (r, _) = store.live(room, now).ok_or(NOT_FOUND)?;
                if let Some(v) = check(r)? {
                    return Ok(Some(v));
                }
                if now >= deadline || *closing.borrow() {
                    return Ok(None);
                }
                (r.bell.subscribe(), deadline.min(r.expires))
            };
            tokio::select! {
                _ = timeout_at(until, bell.changed()) => {}
                _ = closing.changed() => {}
            }
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Ошибка API: код ответа и короткое имя причины в JSON `{"error": …}`.
#[derive(Clone, Copy)]
struct ApiError(StatusCode, &'static str);

const BAD_ID: ApiError = ApiError(StatusCode::BAD_REQUEST, "bad_id");
const BAD_BODY: ApiError = ApiError(StatusCode::BAD_REQUEST, "bad_body");
const BAD_TTL: ApiError = ApiError(StatusCode::BAD_REQUEST, "bad_ttl");
const BAD_OWNER: ApiError = ApiError(StatusCode::BAD_REQUEST, "bad_owner");
const FORBIDDEN: ApiError = ApiError(StatusCode::FORBIDDEN, "forbidden");
const NOT_FOUND: ApiError = ApiError(StatusCode::NOT_FOUND, "not_found");
const CONFLICT: ApiError = ApiError(StatusCode::CONFLICT, "room_taken");
const TOO_LARGE: ApiError = ApiError(StatusCode::PAYLOAD_TOO_LARGE, "too_large");
const TOO_MANY_OFFERS: ApiError = ApiError(StatusCode::TOO_MANY_REQUESTS, "too_many_offers");
const RATE_LIMITED: ApiError = ApiError(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
const ROOMS_FULL: ApiError = ApiError(StatusCode::SERVICE_UNAVAILABLE, "rooms_full");
const STORAGE_FULL: ApiError = ApiError(StatusCode::SERVICE_UNAVAILABLE, "storage_full");

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut resp = (self.0, Json(json!({ "error": self.1 }))).into_response();
        if self.0 == StatusCode::TOO_MANY_REQUESTS && self.1 == RATE_LIMITED.1 {
            resp.headers_mut().insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        }
        resp
    }
}

/// Все маршруты под `/share/api/v1`.
pub fn router(state: Arc<AppState>) -> Router {
    let limit = middleware::from_fn_with_state(state.clone(), rate_limit);
    let api = Router::new()
        .route("/healthz", get(healthz))
        .route("/rooms/{room}", put(put_room).layer(limit.clone()).delete(delete_room))
        .route("/rooms/{room}/offers", post(post_offer).layer(limit).get(get_offers))
        .route("/rooms/{room}/answers/{sid}", put(put_answer).get(get_answer))
        .with_state(state);
    Router::new()
        .nest("/share/api/v1", api)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn(check_length))
        .layer(middleware::from_fn(no_store))
}

/// `Cache-Control: no-store` на каждом ответе, включая ошибки и 404.
async fn no_store(req: Request, next: Next) -> Response {
    let mut resp = next.run(req).await;
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// Заранее объявленная длина больше предела — 413 без чтения тела. Тела без
/// длины (chunked) ограничивает `DefaultBodyLimit` при чтении.
async fn check_length(req: Request, next: Next) -> Response {
    let len = req.headers().get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok());
    if len.and_then(|v| v.parse::<usize>().ok()).is_some_and(|n| n > MAX_BODY) {
        return TOO_LARGE.into_response();
    }
    next.run(req).await
}

/// Не больше `rate_limit` запросов в окно с одного IP на `PUT /rooms` и
/// `POST …/offers` (счётчик общий).
async fn rate_limit(State(s): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    if !s.allow(client_ip(req.headers(), peer)) {
        return RATE_LIMITED.into_response();
    }
    next.run(req).await
}

/// IP клиента: `X-Real-IP`, затем первый адрес `X-Forwarded-For` (служба стоит
/// за nginx, который их выставляет), иначе адрес соединения. Для IPv6 берётся
/// сеть /64: один клиент владеет всей сетью и иначе обходил бы лимит.
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> IpAddr {
    let header_ip = |name: &str, first: bool| -> Option<IpAddr> {
        let v = headers.get(name)?.to_str().ok()?;
        let v = if first { v.split(',').next()? } else { v };
        v.trim().parse().ok()
    };
    let ip = header_ip("x-real-ip", false)
        .or_else(|| header_ip("x-forwarded-for", true))
        .or(peer.map(|p| p.ip()))
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    match ip.to_canonical() {
        IpAddr::V6(v6) => {
            let mut o = v6.octets();
            o[8..].fill(0);
            IpAddr::V6(o.into())
        }
        v4 => v4,
    }
}

fn check_id(s: &str) -> Result<(), ApiError> {
    let ok =
        (8..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if ok { Ok(()) } else { Err(BAD_ID) }
}

/// SHA-256 от заголовка `X-Owner`; `None` — заголовка нет или он негодный.
fn owner_hash(headers: &HeaderMap) -> Option<[u8; 32]> {
    let v = headers.get("x-owner")?.as_bytes();
    OWNER_LEN.contains(&v.len()).then(|| Sha256::digest(v).into())
}

fn is_owner(room: &Room, owner: Option<[u8; 32]>) -> bool {
    owner.is_some_and(|o| ct_eq(&o, &room.owner))
}

/// Сравнение за постоянное время.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|_| BAD_BODY)
}

/// Тело `{"msg": "…"}`; сообщение для службы непрозрачно, лишь не пустое.
fn parse_msg(body: &[u8]) -> Result<String, ApiError> {
    #[derive(Deserialize)]
    struct Body {
        msg: String,
    }
    let Body { msg } = parse(body)?;
    if msg.is_empty() { Err(BAD_BODY) } else { Ok(msg) }
}

fn new_sid() -> String {
    let b: [u8; 12] = rand::random();
    b.iter().map(|x| ID_ALPHABET[(x & 63) as usize] as char).collect()
}

fn wait_of(wait: Option<u64>) -> Duration {
    Duration::from_secs(wait.unwrap_or(DEFAULT_WAIT).min(MAX_WAIT))
}

#[derive(Deserialize)]
struct PollQuery {
    after: Option<u64>,
    wait: Option<u64>,
}

/// `GET /healthz`: для мониторинга и установщика; число комнат — только счётчик.
async fn healthz(State(s): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "rooms": s.rooms() }))
}

/// `PUT /rooms/{room}`: создать комнату или продлить свою.
async fn put_room(
    State(s): State<Arc<AppState>>,
    Path(room): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    #[derive(Deserialize)]
    struct Body {
        ttl: u64,
    }
    check_id(&room)?;
    let owner = owner_hash(&headers).ok_or(BAD_OWNER)?;
    let Body { ttl } = parse(&body)?;
    if !(MIN_TTL..=MAX_TTL).contains(&ttl) {
        return Err(BAD_TTL);
    }
    let now = Instant::now();
    let expires = now + Duration::from_secs(ttl);
    let expires_unix = unix_now() + ttl;
    let mut store = lock(&s.store);
    if let Some((r, _)) = store.live(&room, now) {
        if !ct_eq(&r.owner, &owner) {
            return Err(CONFLICT);
        }
        r.expires = expires;
        r.expires_unix = expires_unix;
    } else {
        if store.rooms.len() >= s.cfg.max_rooms {
            store.purge(now);
        }
        if store.rooms.len() >= s.cfg.max_rooms {
            return Err(ROOMS_FULL);
        }
        eprintln!("комната {}… создана", &room[..6]);
        store.rooms.insert(room, Room::new(owner, expires, expires_unix));
    }
    Ok(Json(json!({ "expires": expires_unix })).into_response())
}

/// `DELETE /rooms/{room}`: владелец закрывает комнату.
async fn delete_room(
    State(s): State<Arc<AppState>>,
    Path(room): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    check_id(&room)?;
    let owner = owner_hash(&headers);
    let mut store = lock(&s.store);
    let (r, _) = store.live(&room, Instant::now()).ok_or(NOT_FOUND)?;
    if !is_owner(r, owner) {
        return Err(FORBIDDEN);
    }
    store.remove(&room, "удалена");
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /rooms/{room}/offers`: браузер кладёт предложение.
async fn post_offer(
    State(s): State<Arc<AppState>>,
    Path(room): Path<String>,
    body: Bytes,
) -> Result<Response, ApiError> {
    check_id(&room)?;
    let msg = parse_msg(&body)?;
    let mut store = lock(&s.store);
    let (r, used) = store.live(&room, Instant::now()).ok_or(NOT_FOUND)?;
    if r.offers.len() >= s.cfg.max_offers {
        return Err(TOO_MANY_OFFERS);
    }
    if *used + msg.len() > s.cfg.max_bytes {
        return Err(STORAGE_FULL);
    }
    let sid = loop {
        let sid = new_sid();
        if !r.offers.iter().any(|o| o.sid == sid) {
            break sid;
        }
    };
    r.seq += 1;
    r.bytes += msg.len();
    *used += msg.len();
    r.offers.push(Offer { seq: r.seq, sid: sid.clone(), msg });
    r.ring();
    Ok((StatusCode::CREATED, Json(json!({ "sid": sid }))).into_response())
}

/// `GET /rooms/{room}/offers?after&wait`: владелец ждёт новые предложения.
async fn get_offers(
    State(s): State<Arc<AppState>>,
    Path(room): Path<String>,
    Query(q): Query<PollQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    check_id(&room)?;
    let owner = owner_hash(&headers);
    let after = q.after.unwrap_or(0);
    let offers = s
        .poll(&room, wait_of(q.wait), |r| {
            if !is_owner(r, owner) {
                return Err(FORBIDDEN);
            }
            let new: Vec<Offer> = r.offers.iter().filter(|o| o.seq > after).cloned().collect();
            Ok((!new.is_empty()).then_some(new))
        })
        .await?
        .unwrap_or_default();
    Ok(Json(json!({ "offers": offers })).into_response())
}

/// `PUT /rooms/{room}/answers/{sid}`: владелец отвечает на предложение.
async fn put_answer(
    State(s): State<Arc<AppState>>,
    Path((room, sid)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    check_id(&room)?;
    check_id(&sid)?;
    let msg = parse_msg(&body)?;
    let owner = owner_hash(&headers);
    let mut store = lock(&s.store);
    let (r, used) = store.live(&room, Instant::now()).ok_or(NOT_FOUND)?;
    if !is_owner(r, owner) {
        return Err(FORBIDDEN);
    }
    if !r.offers.iter().any(|o| o.sid == sid) {
        return Err(NOT_FOUND);
    }
    // Повторный ответ заменяет прежний.
    let old = r.answers.get(&sid).map_or(0, String::len);
    if *used - old + msg.len() > s.cfg.max_bytes {
        return Err(STORAGE_FULL);
    }
    *used = *used - old + msg.len();
    r.bytes = r.bytes - old + msg.len();
    r.answers.insert(sid, msg);
    r.ring();
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /rooms/{room}/answers/{sid}?wait`: браузер ждёт ответ.
async fn get_answer(
    State(s): State<Arc<AppState>>,
    Path((room, sid)): Path<(String, String)>,
    Query(q): Query<PollQuery>,
) -> Result<Response, ApiError> {
    check_id(&room)?;
    check_id(&sid)?;
    let answer = s
        .poll(&room, wait_of(q.wait), |r| {
            if !r.offers.iter().any(|o| o.sid == sid) {
                return Err(NOT_FOUND);
            }
            Ok(r.answers.get(&sid).cloned())
        })
        .await?;
    Ok(match answer {
        Some(msg) => Json(json!({ "msg": msg })).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    })
}
