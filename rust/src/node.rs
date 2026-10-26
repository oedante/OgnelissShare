//! Узел Ogneliss Share: соединения, «Мои устройства», сопряжение по QR,
//! присутствие в сети.
//!
//! Главное правило безопасности: входящее соединение принимается, только
//! если ключ собеседника есть в списке связанных устройств или разовых
//! сеансов. Исключение — пока на экране показан QR-код: тогда несколько
//! неизвестных устройств могут подключиться, но им разрешено ровно одно
//! действие — предъявить секрет из этого кода в течение 10 секунд.
//! Всё остальное отклоняется ещё на этапе рукопожатия (код 403), поэтому
//! посторонний не может ни прислать файл, ни узнать, что здесь кто-то есть.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use iroh::endpoint::{
    AfterHandshakeOutcome, Connection, ConnectionError, EndpointHooks, RecvStream, SendStream, Side, VarInt,
    presets,
};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, Watcher};
use serde_json::{Value, json};
use tokio::sync::{Notify, oneshot};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::events::Events;
use crate::pairing::{PairLink, PairMode, decode_token, pair_code};
use crate::proto::{ALPN, CODE_BUSY, CODE_NOT_PAIRED, CODE_REMOVED, Msg, read_msg, write_msg};
use crate::store::{PeerRecord, Saved, Store};
use crate::transfer::Transfer;
use crate::util::{clean_name, clean_platform, ct_eq, now_unix, random_id, short_code};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Сколько живёт QR-код.
const INVITE_TTL: Duration = Duration::from_secs(5 * 60);
/// Сколько ждём решения «Подключить?» на показывающем устройстве.
const PAIR_DECISION_TTL: Duration = Duration::from_secs(120);
/// Разовый сеанс без действий закрывается сам.
const SESSION_IDLE: Duration = Duration::from_secs(30 * 60);
/// Разовый сеанс без соединения убирается из списка через это время.
const SESSION_GRACE: Duration = Duration::from_secs(90);
/// Одновременно не больше стольких неизвестных устройств с QR-кодом.
const MAX_GUESTS: usize = 4;
/// Предел ожидания установки соединения.
pub(crate) const DIAL_TIMEOUT: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------- настройки

#[derive(Clone, Debug)]
pub enum Relays {
    /// Общедоступные ретрансляторы n0 — пока не развёрнут свой.
    Default,
    /// Свои ретрансляторы, например `https://relay.ogneliss.ru`.
    Custom(Vec<RelayUrl>),
    /// Без ретранслятора: только прямые адреса (тесты, локальная сеть).
    Disabled,
}

impl Relays {
    /// Ретрансляторы задаются при сборке переменной `OGNELISS_RELAYS`
    /// (адреса через запятую). Не задана — общедоступные ретрансляторы n0.
    pub fn from_build() -> Self {
        match option_env!("OGNELISS_RELAYS") {
            Some(list) if !list.trim().is_empty() => {
                let urls: Vec<RelayUrl> = list.split(',').filter_map(|u| u.trim().parse().ok()).collect();
                if urls.is_empty() { Relays::Default } else { Relays::Custom(urls) }
            }
            _ => Relays::Default,
        }
    }

    fn label(&self) -> String {
        match self {
            Relays::Default => "n0".into(),
            Relays::Custom(urls) => urls.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(","),
            Relays::Disabled => "off".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    /// Папка данных приложения: ключ и список устройств.
    pub data_dir: PathBuf,
    /// Куда сохранять полученные файлы.
    pub download_dir: PathBuf,
    /// Имя устройства по умолчанию (из системы).
    pub device_name: String,
    pub platform: String,
    pub relays: Relays,
    /// Поиск связанных устройств в локальной сети без интернета.
    pub mdns: bool,
}

// ---------------------------------------------------------------- шлюз

/// Решение о входящем соединении принимается сразу после рукопожатия TLS,
/// когда ключ собеседника уже проверен.
#[derive(Debug, Default)]
struct Gate {
    allowed: RwLock<HashSet<EndpointId>>,
    /// До какого момента показан QR-код.
    open_until: Mutex<Option<Instant>>,
    guests: AtomicUsize,
}

impl Gate {
    fn set_allowed(&self, ids: impl IntoIterator<Item = EndpointId>) {
        if let Ok(mut a) = self.allowed.write() {
            *a = ids.into_iter().collect();
        }
    }

    fn is_allowed(&self, id: &EndpointId) -> bool {
        self.allowed.read().map(|a| a.contains(id)).unwrap_or(false)
    }

    fn open(&self, until: Option<Instant>) {
        if let Ok(mut o) = self.open_until.lock() {
            *o = until;
        }
    }

    fn invites_open(&self) -> bool {
        self.open_until.lock().map(|o| o.is_some_and(|t| t > Instant::now())).unwrap_or(false)
    }

    fn check(&self, conn: &Connection) -> AfterHandshakeOutcome {
        if conn.alpn() != ALPN {
            return reject(CODE_NOT_PAIRED, b"protocol");
        }
        // Исходящие соединения мы открываем сами — к ключу, который выбрали.
        if conn.side() == Side::Client {
            return AfterHandshakeOutcome::Accept;
        }
        if self.is_allowed(&conn.remote_id()) {
            return AfterHandshakeOutcome::Accept;
        }
        if self.invites_open() && self.guests.load(Ordering::SeqCst) < MAX_GUESTS {
            return AfterHandshakeOutcome::Accept;
        }
        reject(CODE_NOT_PAIRED, b"not paired")
    }
}

fn reject(code: u32, reason: &[u8]) -> AfterHandshakeOutcome {
    AfterHandshakeOutcome::Reject { error_code: VarInt::from_u32(code), reason: reason.to_vec() }
}

/// Хранит только шлюз, а не сам Endpoint — иначе получится цикл ссылок.
#[derive(Debug)]
struct GateHook(Arc<Gate>);

impl EndpointHooks for GateHook {
    fn after_handshake<'a>(
        &'a self,
        conn: &'a Connection,
    ) -> impl Future<Output = AfterHandshakeOutcome> + Send + 'a {
        let verdict = self.0.check(conn);
        async move { verdict }
    }
}

/// Уменьшает счётчик гостей при выходе из обработки.
struct GuestSlot<'a>(&'a AtomicUsize);

impl Drop for GuestSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------- состояние

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// «Мои устройства»: постоянная связь, файлы принимаются без вопросов.
    Paired,
    /// Разовый обмен: каждую передачу файлов получатель подтверждает.
    Session,
}

#[derive(Default)]
pub(crate) struct Link {
    pub conns: Vec<Connection>,
    pub dialing: bool,
    pub fails: u32,
    pub next_try: Option<Instant>,
}

impl Link {
    fn prune(&mut self) {
        self.conns.retain(|c| c.close_reason().is_none());
    }

    fn live(&mut self) -> Option<Connection> {
        self.prune();
        self.conns.last().cloned()
    }
}

pub(crate) struct Session {
    pub name: String,
    pub platform: String,
    pub last_active: Instant,
    pub disconnected_at: Option<Instant>,
}

pub(crate) struct Invite {
    pub mode: PairMode,
    pub token: [u8; 16],
    pub expires: Instant,
    /// Кто воспользовался кодом и чем закончилось.
    pub joined: Option<Joined>,
}

pub(crate) struct Joined {
    pub name: String,
    pub platform: String,
    /// pending — ждём решения; done — связаны; declined — отклонено.
    pub state: &'static str,
}

pub(crate) struct PairRequest {
    pub id: String,
    pub peer: EndpointId,
    pub name: String,
    pub platform: String,
    pub code: String,
    pub reply: Option<oneshot::Sender<bool>>,
    pub created: Instant,
}

pub(crate) struct State {
    pub saved: Saved,
    pub download_dir: PathBuf,
    pub links: HashMap<EndpointId, Link>,
    pub sessions: HashMap<EndpointId, Session>,
    pub invite: Option<Invite>,
    pub pair_requests: Vec<PairRequest>,
    pub transfers: Vec<Transfer>,
    pub active: bool,
    /// Соединения, по которым идут передачи (stable_id → число передач):
    /// такие не закрываем при уборке дублей.
    pub busy_conns: HashMap<usize, u32>,
}

impl State {
    pub fn kind(&self, id: &EndpointId) -> Option<Kind> {
        if self.saved.peers.iter().any(|p| &p.id == id) {
            Some(Kind::Paired)
        } else if self.sessions.contains_key(id) {
            Some(Kind::Session)
        } else {
            None
        }
    }

    pub fn peer_name(&self, id: &EndpointId) -> String {
        if let Some(p) = self.saved.peers.iter().find(|p| &p.id == id) {
            return p.name.clone();
        }
        if let Some(s) = self.sessions.get(id) {
            return s.name.clone();
        }
        "Устройство".to_string()
    }

    fn is_online(&mut self, id: &EndpointId) -> bool {
        self.links.get_mut(id).and_then(|l| l.live()).is_some()
    }
}

// ---------------------------------------------------------------- узел

pub struct Node {
    pub ep: Endpoint,
    pub events: Arc<Events>,
    pub(crate) cfg: Config,
    store: Store,
    gate: Arc<Gate>,
    pub(crate) state: Mutex<State>,
    pub(crate) cancel: CancellationToken,
    kick: Notify,
}

impl Node {
    pub async fn start(cfg: Config) -> Result<Arc<Node>> {
        let store = Store::open(&cfg.data_dir)?;
        let key = store.load_or_create_key()?;
        let saved = store.load_state();
        let gate = Arc::new(Gate::default());
        gate.set_allowed(saved.peers.iter().map(|p| p.id));

        let mut builder = Endpoint::builder(presets::Minimal)
            .secret_key(key)
            .alpns(vec![ALPN.to_vec()])
            .hooks(GateHook(gate.clone()));
        builder = match &cfg.relays {
            Relays::Default => builder.relay_mode(RelayMode::Default),
            Relays::Custom(urls) => builder.relay_mode(RelayMode::custom(urls.clone())),
            Relays::Disabled => builder.relay_mode(RelayMode::Disabled),
        };
        let ep = builder.bind().await.context("bind")?;
        let events = Arc::new(Events::default());

        // Локальная сеть: связанные устройства находятся и без интернета.
        // Не получилось (нет сети, запрет multicast) — работаем без этого.
        #[cfg(not(target_os = "ios"))]
        if cfg.mdns {
            match iroh_mdns_address_lookup::MdnsAddressLookup::builder()
                .service_name("ogshare")
                .build(ep.id())
            {
                Ok(mdns) => match ep.address_lookup() {
                    Ok(lookup) => lookup.add(mdns),
                    Err(e) => events.log(format!("mdns: {e}")),
                },
                Err(e) => events.log(format!("mdns: {e}")),
            }
        }

        let download_dir = if saved.download_dir.trim().is_empty() {
            cfg.download_dir.clone()
        } else {
            PathBuf::from(&saved.download_dir)
        };
        let node = Arc::new(Node {
            ep,
            events,
            store,
            gate,
            state: Mutex::new(State {
                saved,
                download_dir: download_dir.clone(),
                links: HashMap::new(),
                sessions: HashMap::new(),
                invite: None,
                pair_requests: Vec::new(),
                transfers: Vec::new(),
                active: true,
                busy_conns: HashMap::new(),
            }),
            cancel: CancellationToken::new(),
            kick: Notify::new(),
            cfg,
        });
        node.events.log(format!(
            "start {} v{} relays={}",
            node.ep.id().fmt_short(),
            VERSION,
            node.cfg.relays.label()
        ));
        tokio::spawn(node.clone().accept_loop());
        tokio::spawn(node.clone().presence_loop());
        tokio::spawn(node.clone().watch_network());
        let roots = [download_dir.join(crate::transfer::STAGE_DIR), node.cfg.data_dir.join("stage")];
        tokio::task::spawn_blocking(move || roots.iter().for_each(|r| crate::transfer::cleanup_stale(r)));
        Ok(node)
    }

    pub async fn shutdown(&self) {
        self.cancel.cancel();
        self.ep.close().await;
    }

    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut guard = match self.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        f(&mut guard)
    }

    pub fn id(&self) -> EndpointId {
        self.ep.id()
    }

    pub fn name(&self) -> String {
        self.with(|s| {
            if s.saved.name.trim().is_empty() {
                clean_name(&self.cfg.device_name)
            } else {
                s.saved.name.clone()
            }
        })
    }

    pub fn download_dir(&self) -> PathBuf {
        self.with(|s| s.download_dir.clone())
    }

    /// Своя папка для полученных файлов; None — вернуть папку по умолчанию.
    pub fn set_download_dir(&self, dir: Option<PathBuf>) {
        self.with(|s| match dir {
            Some(d) => {
                s.saved.download_dir = d.to_string_lossy().into_owned();
                s.download_dir = d;
            }
            None => {
                s.saved.download_dir.clear();
                s.download_dir = self.cfg.download_dir.clone();
            }
        });
        self.save();
        self.events.changed();
    }

    /// Куда принимать недокачанное: рядом с папкой загрузок (тогда перенос
    /// на место — простое переименование), а если там нельзя создать
    /// служебную папку — в данные приложения.
    pub(crate) fn stage_root(&self) -> PathBuf {
        let root = self.download_dir().join(crate::transfer::STAGE_DIR);
        if std::fs::create_dir_all(&root).is_ok() { root } else { self.cfg.data_dir.join("stage") }
    }

    fn save(&self) {
        let saved = self.with(|s| s.saved.clone());
        if let Err(e) = self.store.save_state(&saved) {
            self.events.log(format!("save: {e:#}"));
        }
    }

    fn sync_gate(&self) {
        let ids: Vec<EndpointId> =
            self.with(|s| s.saved.peers.iter().map(|p| p.id).chain(s.sessions.keys().copied()).collect());
        self.gate.set_allowed(ids);
    }

    pub(crate) fn kind(&self, id: &EndpointId) -> Option<Kind> {
        self.with(|s| s.kind(id))
    }

    pub(crate) fn peer_name(&self, id: &EndpointId) -> String {
        self.with(|s| s.peer_name(id))
    }

    pub(crate) fn touch_session(&self, id: &EndpointId) {
        self.with(|s| {
            if let Some(sess) = s.sessions.get_mut(id) {
                sess.last_active = Instant::now();
            }
        });
    }

    /// Разбудить цикл присутствия: попробовать связаться прямо сейчас.
    pub fn refresh(&self) {
        self.with(|s| {
            for link in s.links.values_mut() {
                link.next_try = None;
                link.fails = 0;
            }
        });
        self.kick.notify_one();
    }

    /// Приложение на экране или в фоне. В фоне не тратим заряд на попытки
    /// дозвониться до выключенных устройств.
    pub fn set_active(&self, active: bool) {
        self.with(|s| s.active = active);
        if active {
            self.refresh();
        }
    }

    pub fn set_name(self: &Arc<Self>, name: &str) {
        let name = name.trim();
        self.with(|s| s.saved.name = if name.is_empty() { String::new() } else { clean_name(name) });
        self.save();
        self.events.changed();
        let conns = self.live_connections();
        let node = self.clone();
        tokio::spawn(async move {
            for c in conns {
                let _ = node.say_hello(&c).await;
            }
        });
    }

    // ------------------------------------------------------------ соединения

    pub(crate) fn live_conn(&self, id: &EndpointId) -> Option<Connection> {
        self.with(|s| s.links.get_mut(id).and_then(|l| l.live()))
    }

    fn live_connections(&self) -> Vec<Connection> {
        self.with(|s| s.links.values_mut().filter_map(|l| l.live()).collect())
    }

    pub(crate) fn mark_busy(&self, conn: &Connection, busy: bool) {
        let id = conn.stable_id();
        self.with(|s| {
            if busy {
                *s.busy_conns.entry(id).or_insert(0) += 1;
            } else if let Some(n) = s.busy_conns.get_mut(&id) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    s.busy_conns.remove(&id);
                }
            }
        });
    }

    /// Адрес для звонка: последний известный, плюс наши ретрансляторы —
    /// у всех устройств один список, поэтому собеседник почти наверняка
    /// подключён к одному из них.
    fn dial_addr(&self, id: &EndpointId) -> EndpointAddr {
        let stored = self.with(|s| s.saved.peers.iter().find(|p| &p.id == id).and_then(|p| p.addr.clone()));
        let mut addr = stored.filter(|a| &a.id == id).unwrap_or_else(|| EndpointAddr::new(*id));
        if let Relays::Custom(urls) = &self.cfg.relays {
            for u in urls {
                addr = addr.with_relay_url(u.clone());
            }
        }
        addr
    }

    /// Готовое соединение с устройством: живое или новое.
    pub(crate) async fn connection_to(self: &Arc<Self>, id: EndpointId) -> Result<Connection> {
        if let Some(c) = self.live_conn(&id) {
            return Ok(c);
        }
        self.dial(id).await
    }

    async fn dial(self: &Arc<Self>, id: EndpointId) -> Result<Connection> {
        let addr = self.dial_addr(&id);
        let conn = match timeout(DIAL_TIMEOUT, self.ep.connect(addr, ALPN)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return Err(anyhow!(e).context("unreachable")),
            Err(_) => bail!("unreachable"),
        };
        self.adopt(conn.clone());
        if let Err(e) = self.say_hello(&conn).await {
            if let Some(code) = close_code(&conn)
                && (code == CODE_NOT_PAIRED as u64 || code == CODE_REMOVED as u64)
            {
                self.forget_by_peer(&id);
                bail!("removed");
            }
            return Err(e.context("hello"));
        }
        Ok(conn)
    }

    /// Взять соединение под обслуживание: принимать входящие потоки.
    fn adopt(self: &Arc<Self>, conn: Connection) {
        let id = conn.remote_id();
        self.with(|s| {
            let link = s.links.entry(id).or_default();
            link.prune();
            link.conns.push(conn.clone());
            link.fails = 0;
            link.next_try = None;
            if let Some(sess) = s.sessions.get_mut(&id) {
                sess.disconnected_at = None;
                sess.last_active = Instant::now();
            }
            if let Some(p) = s.saved.peers.iter_mut().find(|p| p.id == id) {
                p.last_seen = now_unix();
            }
        });
        self.events.changed();
        tokio::spawn(self.clone().serve(conn));
    }

    async fn serve(self: Arc<Self>, conn: Connection) {
        let peer = conn.remote_id();
        loop {
            let next = tokio::select! {
                _ = self.cancel.cancelled() => break,
                r = conn.accept_bi() => r,
            };
            match next {
                Ok((send, recv)) => {
                    let node = self.clone();
                    let c = conn.clone();
                    tokio::spawn(async move { node.handle_stream(c, send, recv).await });
                }
                Err(_) => break,
            }
        }
        let code = close_code(&conn);
        self.with(|s| {
            if let Some(link) = s.links.get_mut(&peer) {
                link.conns.retain(|c| c.stable_id() != conn.stable_id());
                link.prune();
                if link.conns.is_empty()
                    && let Some(sess) = s.sessions.get_mut(&peer)
                {
                    sess.disconnected_at = Some(Instant::now());
                }
            }
            if let Some(p) = s.saved.peers.iter_mut().find(|p| p.id == peer) {
                p.last_seen = now_unix();
            }
        });
        if code == Some(CODE_REMOVED as u64) || code == Some(CODE_NOT_PAIRED as u64) {
            self.forget_by_peer(&peer);
        }
        self.events.changed();
        self.kick.notify_one();
    }

    async fn accept_loop(self: Arc<Self>) {
        loop {
            let incoming = tokio::select! {
                _ = self.cancel.cancelled() => break,
                i = self.ep.accept() => i,
            };
            let Some(incoming) = incoming else { break };
            let node = self.clone();
            tokio::spawn(async move {
                match incoming.await {
                    Ok(conn) => node.on_incoming(conn).await,
                    Err(e) => node.events.log(format!("incoming: {e}")),
                }
            });
        }
    }

    async fn on_incoming(self: Arc<Self>, conn: Connection) {
        let peer = conn.remote_id();
        if self.kind(&peer).is_some() {
            self.adopt(conn);
        } else {
            self.serve_guest(conn).await;
        }
    }

    async fn handle_stream(self: Arc<Self>, conn: Connection, mut send: SendStream, mut recv: RecvStream) {
        let peer = conn.remote_id();
        if self.kind(&peer).is_none() {
            conn.close(VarInt::from_u32(CODE_NOT_PAIRED), b"not paired");
            return;
        }
        self.touch_session(&peer);
        let msg = match timeout(Duration::from_secs(30), read_msg(&mut recv)).await {
            Ok(Ok(m)) => m,
            _ => {
                let _ = send.reset(VarInt::from_u32(crate::proto::CODE_CANCELLED));
                return;
            }
        };
        match msg {
            Msg::Hello { name, platform, addr, .. } => {
                self.update_peer(&peer, &name, &platform, addr);
                if write_msg(&mut send, &self.hello_msg()).await.is_ok() {
                    let _ = send.finish();
                    let _ = send.stopped().await;
                }
            }
            Msg::Offer { id, files, total } => {
                self.mark_busy(&conn, true);
                self.clone().receive_offer(peer, send, recv, id, files, total).await;
                self.mark_busy(&conn, false);
            }
            Msg::Text { id, text } => {
                self.receive_text(peer, send, id, text).await;
            }
            _ => {
                let _ = send.reset(VarInt::from_u32(crate::proto::CODE_CANCELLED));
            }
        }
    }

    // ------------------------------------------------------------ приветствие

    fn hello_msg(&self) -> Msg {
        Msg::Hello {
            name: self.name(),
            platform: self.cfg.platform.clone(),
            version: VERSION.to_string(),
            addr: Some(self.ep.addr()),
        }
    }

    pub(crate) async fn say_hello(self: &Arc<Self>, conn: &Connection) -> Result<()> {
        let (mut send, mut recv) = conn.open_bi().await?;
        write_msg(&mut send, &self.hello_msg()).await?;
        send.finish()?;
        match timeout(Duration::from_secs(15), read_msg(&mut recv)).await {
            Ok(Ok(Msg::Hello { name, platform, addr, .. })) => {
                self.update_peer(&conn.remote_id(), &name, &platform, addr);
                Ok(())
            }
            Ok(Ok(_)) => bail!("protocol"),
            Ok(Err(e)) => Err(e),
            Err(_) => bail!("timeout"),
        }
    }

    fn update_peer(&self, id: &EndpointId, name: &str, platform: &str, addr: Option<EndpointAddr>) {
        let name = clean_name(name);
        let platform = clean_platform(platform);
        let paired = self.with(|s| {
            if let Some(p) = s.saved.peers.iter_mut().find(|p| &p.id == id) {
                p.name = name.clone();
                p.platform = platform.clone();
                p.last_seen = now_unix();
                if let Some(a) = addr.filter(|a| &a.id == id) {
                    p.addr = Some(a);
                }
                true
            } else {
                if let Some(sess) = s.sessions.get_mut(id) {
                    sess.name = name.clone();
                    sess.platform = platform.clone();
                }
                false
            }
        });
        if paired {
            self.save();
        }
        self.events.changed();
    }

    // ------------------------------------------------------------ присутствие

    async fn presence_loop(self: Arc<Self>) {
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(3)) => {}
                _ = self.kick.notified() => {}
            }
            self.housekeeping();
            if !self.with(|s| s.active) {
                continue;
            }
            let now = Instant::now();
            let to_dial: Vec<EndpointId> = self.with(|s| {
                let mut ids: Vec<EndpointId> = s.saved.peers.iter().map(|p| p.id).collect();
                ids.extend(s.sessions.keys().copied());
                ids.into_iter()
                    .filter(|id| {
                        let link = s.links.entry(*id).or_default();
                        if link.dialing || link.live().is_some() {
                            return false;
                        }
                        if link.next_try.is_some_and(|t| t > now) {
                            return false;
                        }
                        link.dialing = true;
                        true
                    })
                    .collect()
            });
            for id in to_dial {
                let node = self.clone();
                tokio::spawn(async move {
                    let res = node.dial(id).await;
                    node.with(|s| {
                        let link = s.links.entry(id).or_default();
                        link.dialing = false;
                        if res.is_err() {
                            link.fails = link.fails.saturating_add(1);
                            link.next_try = Some(Instant::now() + backoff(link.fails));
                        }
                    });
                    if res.is_ok() {
                        node.events.changed();
                    }
                });
            }
            self.dedupe();
        }
    }

    /// Убрать лишние соединения с одним устройством. Закрывает только свои
    /// исходящие и только свободные; правило одно на обеих сторонах, так
    /// что они не закроют одно и то же соединение навстречу друг другу.
    fn dedupe(&self) {
        let me = self.id();
        let to_close: Vec<Connection> = self.with(|s| {
            let busy = s.busy_conns.clone();
            let mut out = Vec::new();
            for (peer, link) in s.links.iter_mut() {
                link.prune();
                if link.conns.len() < 2 {
                    continue;
                }
                let has_incoming = link.conns.iter().any(|c| c.side() == Side::Server);
                let outgoing: Vec<Connection> =
                    link.conns.iter().filter(|c| c.side() == Side::Client).cloned().collect();
                let keep_newest_out = !(has_incoming && me.as_bytes() > peer.as_bytes());
                for (i, c) in outgoing.iter().enumerate() {
                    let is_newest = i + 1 == outgoing.len();
                    if (keep_newest_out && is_newest) || busy.contains_key(&c.stable_id()) {
                        continue;
                    }
                    out.push(c.clone());
                }
                let closing: HashSet<usize> = out.iter().map(|c| c.stable_id()).collect();
                link.conns.retain(|c| !closing.contains(&c.stable_id()));
            }
            out
        });
        for c in to_close {
            c.close(VarInt::from_u32(0), b"duplicate");
        }
    }

    fn housekeeping(&self) {
        let now = Instant::now();
        let mut changed = false;
        let mut closed_sessions = Vec::new();
        self.with(|s| {
            if s.invite.as_ref().is_some_and(|i| i.expires <= now) {
                s.invite = None;
                changed = true;
            }
            let before = s.pair_requests.len();
            s.pair_requests.retain(|r| now.duration_since(r.created) < PAIR_DECISION_TTL);
            changed |= before != s.pair_requests.len();
            let active_peers: HashSet<EndpointId> =
                s.transfers.iter().filter(|t| !t.is_finished()).map(|t| t.peer).collect();
            let expired: Vec<EndpointId> = s
                .sessions
                .iter()
                .filter(|(id, sess)| {
                    !active_peers.contains(*id)
                        && (now.duration_since(sess.last_active) > SESSION_IDLE
                            || sess.disconnected_at.is_some_and(|t| now.duration_since(t) > SESSION_GRACE))
                })
                .map(|(id, _)| *id)
                .collect();
            for id in expired {
                s.sessions.remove(&id);
                if let Some(link) = s.links.remove(&id) {
                    closed_sessions.extend(link.conns);
                }
                changed = true;
            }
        });
        let open = self.with(|s| s.invite.as_ref().map(|i| i.expires));
        self.gate.open(open);
        if !closed_sessions.is_empty() {
            self.sync_gate();
            for c in closed_sessions {
                c.close(VarInt::from_u32(CODE_REMOVED), b"session ended");
            }
        }
        if changed {
            self.events.changed();
        }
    }

    async fn watch_network(self: Arc<Self>) {
        let mut relay = self.ep.home_relay_status();
        let mut addr = self.ep.watch_addr();
        let mut last_hello = Instant::now();
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => return,
                r = relay.updated() => {
                    if r.is_err() { return; }
                    self.events.changed();
                }
                r = addr.updated() => {
                    if r.is_err() { return; }
                    self.events.changed();
                    // Свежие адреса — собеседникам, не чаще раза в 10 секунд.
                    if last_hello.elapsed() > Duration::from_secs(10) {
                        last_hello = Instant::now();
                        let node = self.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_secs(3)).await;
                            for c in node.live_connections() {
                                let _ = node.say_hello(&c).await;
                            }
                        });
                    }
                }
            }
        }
    }

    pub fn is_online(&self) -> bool {
        self.ep.home_relay_status().get().iter().any(|s| s.is_connected())
    }

    // ------------------------------------------------------------ сопряжение

    /// Показать QR-код: постоянная связь («pair») или разовый обмен («once»).
    pub fn create_invite(&self, mode: PairMode) -> Value {
        let token: [u8; 16] = rand::random();
        let expires = Instant::now() + INVITE_TTL;
        self.with(|s| s.invite = Some(Invite { mode, token, expires, joined: None }));
        self.gate.open(Some(expires));
        self.events.changed();
        self.invite_json().unwrap_or(Value::Null)
    }

    pub fn cancel_invite(&self) {
        self.with(|s| s.invite = None);
        self.gate.open(None);
        self.events.changed();
    }

    fn invite_link(&self, mode: PairMode, token: [u8; 16]) -> PairLink {
        let addr = self.ep.addr();
        PairLink {
            mode,
            id: self.id(),
            token,
            name: self.name(),
            platform: self.cfg.platform.clone(),
            relay: addr.relay_urls().next().cloned(),
            addrs: addr.ip_addrs().copied().take(6).collect(),
        }
    }

    fn invite_json(&self) -> Option<Value> {
        let (mode, token, expires, joined) = self.with(|s| {
            s.invite.as_ref().map(|i| {
                (
                    i.mode,
                    i.token,
                    i.expires,
                    i.joined.as_ref().map(|j| (j.name.clone(), j.platform.clone(), j.state)),
                )
            })
        })?;
        let link = self.invite_link(mode, token);
        let left = expires.saturating_duration_since(Instant::now()).as_secs();
        Some(json!({
            "mode": mode.as_str(),
            "uri": link.to_uri(),
            "expires_in": left,
            "joined": joined.map(|(name, platform, state)| json!({
                "name": name, "platform": platform, "state": state,
            })),
        }))
    }

    /// Отсканировали чужой QR-код (или вставили ссылку).
    pub fn use_invite(self: &Arc<Self>, uri: &str) -> Result<Value> {
        let link = PairLink::parse(uri).map_err(|_| anyhow!("bad_link"))?;
        if link.id == self.id() {
            bail!("own_link");
        }
        let code = pair_code(&link.id, &self.id(), &link.token);
        let info = json!({
            "peer": link.id.to_string(),
            "name": link.name,
            "platform": link.platform,
            "mode": link.mode.as_str(),
            "code": code,
        });
        let node = self.clone();
        tokio::spawn(async move {
            let res = node.join(&link).await;
            let event = match res {
                Ok(name) => json!({
                    "type": "join_result", "ok": true, "peer": link.id.to_string(),
                    "name": name, "mode": link.mode.as_str(),
                }),
                Err(e) => {
                    node.events.log(format!("join: {e:#}"));
                    json!({
                        "type": "join_result", "ok": false, "peer": link.id.to_string(),
                        "name": link.name, "mode": link.mode.as_str(), "error": error_code(&e),
                    })
                }
            };
            node.events.emit(event);
        });
        Ok(info)
    }

    async fn join(self: &Arc<Self>, link: &PairLink) -> Result<String> {
        let conn = match timeout(Duration::from_secs(30), self.ep.connect(link.endpoint_addr(), ALPN)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return Err(anyhow!(e).context("unreachable")),
            Err(_) => bail!("unreachable"),
        };
        let result = async {
            let (mut send, mut recv) = conn.open_bi().await?;
            write_msg(
                &mut send,
                &Msg::Pair {
                    token: link.token_str(),
                    name: self.name(),
                    platform: self.cfg.platform.clone(),
                    mode: link.mode.as_str().to_string(),
                },
            )
            .await?;
            send.finish()?;
            match timeout(PAIR_DECISION_TTL + Duration::from_secs(30), read_msg(&mut recv)).await {
                Ok(r) => r,
                Err(_) => bail!("timeout"),
            }
        }
        .await;
        let reply = match result {
            Ok(r) => r,
            Err(e) => {
                return match close_code(&conn) {
                    Some(c) if c == CODE_NOT_PAIRED as u64 => Err(e.context("expired")),
                    Some(c) if c == CODE_BUSY as u64 => Err(e.context("busy")),
                    _ => Err(e.context("unreachable")),
                };
            }
        };
        match reply {
            Msg::PairOk { name, platform } => {
                let name = clean_name(&name);
                match link.mode {
                    PairMode::Pair => self.add_paired(link.id, &name, &platform),
                    PairMode::Once => self.add_session(link.id, &name, &platform),
                }
                self.adopt(conn.clone());
                let _ = self.say_hello(&conn).await;
                Ok(name)
            }
            Msg::PairDenied { reason } => {
                conn.close(VarInt::from_u32(0), b"");
                Err(anyhow!(reason))
            }
            _ => bail!("protocol"),
        }
    }

    /// Неизвестное устройство подключилось, пока показан QR-код.
    async fn serve_guest(self: Arc<Self>, conn: Connection) {
        let count = self.gate.guests.fetch_add(1, Ordering::SeqCst) + 1;
        let _slot = GuestSlot(&self.gate.guests);
        if count > MAX_GUESTS || !self.gate.invites_open() {
            conn.close(VarInt::from_u32(CODE_BUSY), b"busy");
            return;
        }
        let peer = conn.remote_id();
        let (mut send, mut recv) = match timeout(Duration::from_secs(10), conn.accept_bi()).await {
            Ok(Ok(s)) => s,
            _ => {
                conn.close(VarInt::from_u32(CODE_NOT_PAIRED), b"not paired");
                return;
            }
        };
        let Ok(Ok(Msg::Pair { token, name, platform, mode })) =
            timeout(Duration::from_secs(10), read_msg(&mut recv)).await
        else {
            conn.close(VarInt::from_u32(CODE_NOT_PAIRED), b"not paired");
            return;
        };
        let name = clean_name(&name);
        let platform = clean_platform(&platform);
        let presented = decode_token(&token).ok();
        let wanted = PairMode::parse(&mode);

        // Код одноразовый: первый, кто его предъявил, его и забирает.
        let accepted = self.with(|s| match (&mut s.invite, presented) {
            (Some(inv), Some(tok))
                if inv.expires > Instant::now()
                    && inv.joined.is_none()
                    && Some(inv.mode) == wanted
                    && ct_eq(&inv.token, &tok) =>
            {
                inv.joined = Some(Joined {
                    name: name.clone(),
                    platform: platform.clone(),
                    state: if inv.mode == PairMode::Pair { "pending" } else { "done" },
                });
                Some((inv.mode, inv.token))
            }
            _ => None,
        });
        let Some((mode, tok)) = accepted else {
            let _ = write_msg(&mut send, &Msg::PairDenied { reason: "expired".into() }).await;
            let _ = send.finish();
            let _ = timeout(Duration::from_secs(2), send.stopped()).await;
            conn.close(VarInt::from_u32(CODE_NOT_PAIRED), b"expired");
            return;
        };
        self.events.changed();

        let approved = match mode {
            PairMode::Once => true,
            PairMode::Pair => {
                let code = pair_code(&self.id(), &peer, &tok);
                let (tx, rx) = oneshot::channel();
                let req_id = random_id();
                self.with(|s| {
                    s.pair_requests.push(PairRequest {
                        id: req_id.clone(),
                        peer,
                        name: name.clone(),
                        platform: platform.clone(),
                        code: code.clone(),
                        reply: Some(tx),
                        created: Instant::now(),
                    })
                });
                self.events.emit(json!({
                    "type": "pair_request", "id": req_id, "peer": peer.to_string(),
                    "name": name, "platform": platform, "code": code,
                }));
                let decision = tokio::select! {
                    r = timeout(PAIR_DECISION_TTL, rx) => matches!(r, Ok(Ok(true))),
                    _ = conn.closed() => false,
                };
                self.with(|s| s.pair_requests.retain(|r| r.id != req_id));
                decision
            }
        };

        if !approved {
            self.with(|s| {
                if let Some(j) = s.invite.as_mut().and_then(|i| i.joined.as_mut()) {
                    j.state = "declined";
                }
            });
            self.events.changed();
            let _ = write_msg(&mut send, &Msg::PairDenied { reason: "declined".into() }).await;
            let _ = send.finish();
            let _ = timeout(Duration::from_secs(2), send.stopped()).await;
            conn.close(VarInt::from_u32(0), b"declined");
            return;
        }

        match mode {
            PairMode::Pair => self.add_paired(peer, &name, &platform),
            PairMode::Once => self.add_session(peer, &name, &platform),
        }
        self.with(|s| {
            if let Some(j) = s.invite.as_mut().and_then(|i| i.joined.as_mut()) {
                j.state = "done";
            }
        });
        let ok = Msg::PairOk { name: self.name(), platform: self.cfg.platform.clone() };
        if write_msg(&mut send, &ok).await.is_err() {
            return;
        }
        let _ = send.finish();
        self.events.emit(json!({
            "type": "joined", "peer": peer.to_string(), "name": name, "mode": mode.as_str(),
        }));
        self.adopt(conn);
    }

    /// Ответ на «Подключить устройство?».
    pub fn respond_pair(&self, request_id: &str, accept: bool) -> Result<()> {
        let tx = self
            .with(|s| s.pair_requests.iter_mut().find(|r| r.id == request_id).and_then(|r| r.reply.take()));
        match tx {
            Some(tx) => {
                let _ = tx.send(accept);
                Ok(())
            }
            None => bail!("not_found"),
        }
    }

    fn add_paired(&self, id: EndpointId, name: &str, platform: &str) {
        let now = now_unix();
        self.with(|s| {
            s.sessions.remove(&id);
            match s.saved.peers.iter_mut().find(|p| p.id == id) {
                Some(p) => {
                    p.name = clean_name(name);
                    p.platform = clean_platform(platform);
                    p.last_seen = now;
                }
                None => s.saved.peers.push(PeerRecord {
                    id,
                    name: clean_name(name),
                    platform: clean_platform(platform),
                    added: now,
                    last_seen: now,
                    addr: None,
                }),
            }
        });
        self.save();
        self.sync_gate();
        self.events.changed();
    }

    fn add_session(&self, id: EndpointId, name: &str, platform: &str) {
        self.with(|s| {
            if s.saved.peers.iter().any(|p| p.id == id) {
                return; // уже среди «Моих устройств»
            }
            s.sessions.insert(
                id,
                Session {
                    name: clean_name(name),
                    platform: clean_platform(platform),
                    last_active: Instant::now(),
                    disconnected_at: None,
                },
            );
        });
        self.sync_gate();
        self.events.changed();
    }

    /// «Забыть устройство» или «Завершить обмен». Собеседник узнаёт об
    /// этом по коду закрытия соединения и тоже убирает нас из списка.
    pub fn remove_peer(&self, id: &EndpointId) -> Result<()> {
        let (known, conns) = self.with(|s| {
            let known = s.kind(id).is_some();
            s.saved.peers.retain(|p| &p.id != id);
            s.sessions.remove(id);
            let conns = s.links.remove(id).map(|l| l.conns).unwrap_or_default();
            (known, conns)
        });
        if !known {
            bail!("not_found");
        }
        self.save();
        self.sync_gate();
        for c in conns {
            c.close(VarInt::from_u32(CODE_REMOVED), b"removed");
        }
        self.cancel_transfers_with(id);
        self.events.changed();
        Ok(())
    }

    /// Собеседник разорвал связь с нами.
    fn forget_by_peer(&self, id: &EndpointId) {
        let (name, removed) = self.with(|s| {
            let name = s.peer_name(id);
            let before = s.saved.peers.len() + s.sessions.len();
            s.saved.peers.retain(|p| &p.id != id);
            s.sessions.remove(id);
            s.links.remove(id);
            (name, before != s.saved.peers.len() + s.sessions.len())
        });
        if removed {
            self.save();
            self.sync_gate();
            self.events.log(format!("removed by {}", id.fmt_short()));
            self.events.emit(json!({"type": "peer_removed", "peer": id.to_string(), "name": name}));
        }
    }

    // ------------------------------------------------------------ снимок

    pub fn snapshot(&self) -> Value {
        let me = json!({
            "id": self.id().to_string(),
            "code": short_code(&self.id()),
            "name": self.name(),
            "platform": self.cfg.platform,
            "online": self.is_online(),
            "relay": self.cfg.relays.label(),
            "download_dir": self.download_dir().to_string_lossy(),
            "download_dir_custom": self.with(|s| !s.saved.download_dir.is_empty()),
            "version": VERSION,
        });
        let (peers, requests, transfers) = self.with(|s| {
            let mut peers = Vec::new();
            let mut list: Vec<PeerRecord> = s.saved.peers.clone();
            list.sort_by_key(|p| p.added);
            for p in list {
                let online = s.is_online(&p.id);
                let dialing = s.links.get(&p.id).is_some_and(|l| l.dialing);
                peers.push(json!({
                    "id": p.id.to_string(),
                    "code": short_code(&p.id),
                    "name": p.name,
                    "platform": p.platform,
                    "kind": "paired",
                    "online": online,
                    "connecting": dialing && !online,
                    "last_seen": p.last_seen,
                }));
            }
            let mut sessions: Vec<(EndpointId, String, String)> =
                s.sessions.iter().map(|(id, x)| (*id, x.name.clone(), x.platform.clone())).collect();
            sessions.sort_by(|a, b| a.1.cmp(&b.1));
            for (id, name, platform) in sessions {
                let online = s.is_online(&id);
                peers.push(json!({
                    "id": id.to_string(),
                    "code": short_code(&id),
                    "name": name,
                    "platform": platform,
                    "kind": "session",
                    "online": online,
                    "connecting": false,
                    "last_seen": now_unix(),
                }));
            }
            let mut requests: Vec<Value> = s
                .pair_requests
                .iter()
                .filter(|r| r.reply.is_some())
                .map(|r| {
                    json!({
                        "id": r.id, "kind": "pair", "peer": r.peer.to_string(),
                        "name": r.name, "platform": r.platform, "code": r.code,
                    })
                })
                .collect();
            requests.extend(s.transfers.iter().filter(|t| t.is_asking()).map(|t| t.request_json()));
            let transfers: Vec<Value> = s.transfers.iter().rev().map(|t| t.to_json()).collect();
            (peers, requests, transfers)
        });
        json!({
            "me": me,
            "peers": peers,
            "invite": self.invite_json(),
            "requests": requests,
            "transfers": transfers,
        })
    }
}

/// Пауза перед новой попыткой связаться: 5, 10, 20, 40, 60 секунд с разбросом.
fn backoff(fails: u32) -> Duration {
    let base = 5u64.saturating_mul(1u64 << fails.saturating_sub(1).min(4)).min(60);
    let jitter: u64 = rand::random::<u64>() % (base * 200 + 1);
    Duration::from_millis(base * 900 + jitter)
}

/// Код, с которым собеседник закрыл соединение.
pub(crate) fn close_code(conn: &Connection) -> Option<u64> {
    match conn.close_reason()? {
        ConnectionError::ApplicationClosed(c) => Some(c.error_code.into_inner()),
        _ => None,
    }
}

/// Короткий код ошибки для интерфейса.
pub(crate) fn error_code(e: &anyhow::Error) -> String {
    const KNOWN: &[&str] = &[
        "unreachable",
        "expired",
        "declined",
        "busy",
        "timeout",
        "removed",
        "bad_link",
        "own_link",
        "not_found",
        "protocol",
        "cancelled",
        "too_big",
        "empty",
        "bad_path",
        "storage",
        "not_started",
        "bad_request",
        "verify",
        "interrupted",
        "read",
    ];
    for cause in e.chain() {
        let s = cause.to_string();
        if KNOWN.contains(&s.as_str()) {
            return s;
        }
    }
    "failed".to_string()
}
