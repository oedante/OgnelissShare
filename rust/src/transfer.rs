//! Передача файлов, папок и текста.
//!
//! Отправитель предлагает список файлов (имя, размер, время изменения).
//! Получатель от «Моих устройств» принимает сразу, от разового собеседника —
//! после подтверждения, и сообщает, сколько байт каждого файла у него уже
//! есть (докачка после обрыва). Затем идут данные; после каждого файла —
//! его хеш BLAKE3, который получатель сверяет со своим.
//!
//! Файлы принимаются во временную папку `.ogneliss-share/<ключ>` внутри папки
//! загрузок и переносятся на место, только когда передача закончилась.
//! Ключ зависит от отправителя и списка файлов, поэтому повторная отправка
//! того же самого продолжает с места обрыва.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use iroh::EndpointId;
use iroh::endpoint::{
    Connection, ConnectionError, ReadError, ReadExactError, RecvStream, SendStream, VarInt, WriteError,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::oneshot;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::node::{Kind, Node, error_code};
use crate::proto::{
    CODE_CANCELLED, CODE_NOT_PAIRED, CODE_REMOVED, FileMeta, MAX_TEXT, Msg, read_msg, write_msg,
};
use crate::util::{ct_eq, now_unix, random_id};

const CHUNK: usize = 256 * 1024;
pub const MAX_FILES: usize = 100_000;
/// Сколько передач держим в списке.
const MAX_LIST: usize = 100;
/// Сколько ждём ответа «Принять?» от получателя.
const ASK_TTL: Duration = Duration::from_secs(120);
/// Временная папка приёма внутри папки загрузок.
pub const STAGE_DIR: &str = ".ogneliss-share";
/// Брошенные недокачанные передачи удаляются через неделю.
const STALE_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);
/// Повторы после обрыва связи посреди передачи.
const RETRIES: u32 = 3;
/// Сколько ждём, пока устройство появится в сети.
const WAIT_ONLINE: Duration = Duration::from_secs(10 * 60);
/// Превью текста в снимке состояния; целиком — командой get_text.
const TEXT_PREVIEW: usize = 2000;

// ---------------------------------------------------------------- модель

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Out,
    In,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum What {
    Files,
    Text,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Status {
    /// Собираем список файлов.
    Preparing,
    /// Устанавливаем соединение.
    Connecting,
    /// Устройство не в сети — ждём его.
    WaitingPeer,
    /// Ждём, пока получатель примет.
    Waiting,
    /// Спрашиваем нашего пользователя «Принять?».
    Asking,
    Active,
    /// Связь оборвалась, восстанавливаем.
    Retrying,
    Done,
    Failed(String),
    Declined,
    Cancelled {
        by_peer: bool,
    },
}

impl Status {
    fn code(&self) -> &'static str {
        match self {
            Status::Preparing => "preparing",
            Status::Connecting => "connecting",
            Status::WaitingPeer => "waiting_peer",
            Status::Waiting => "waiting",
            Status::Asking => "asking",
            Status::Active => "active",
            Status::Retrying => "retrying",
            Status::Done => "done",
            Status::Failed(_) => "failed",
            Status::Declined => "declined",
            Status::Cancelled { .. } => "cancelled",
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(self, Status::Done | Status::Failed(_) | Status::Declined | Status::Cancelled { .. })
    }
}

pub struct Transfer {
    /// Наш номер передачи (для интерфейса).
    pub id: String,
    /// Номер у отправителя: по нему узнаём повторную отправку.
    pub wire_id: String,
    pub peer: EndpointId,
    pub peer_name: String,
    pub dir: Dir,
    pub what: What,
    /// Имена верхнего уровня: «Фото», «отчёт.pdf».
    pub names: Vec<String>,
    pub count: usize,
    pub total: u64,
    pub done: u64,
    pub status: Status,
    pub text: Option<String>,
    pub saved_to: Option<PathBuf>,
    /// saved_to — папка (несколько файлов или папка целиком).
    pub saved_is_dir: bool,
    pub failed_files: usize,
    pub started: u64,
    pub finished: u64,
    /// Что отправляли (для «Повторить»).
    pub paths: Vec<PathBuf>,
    pub cancel: CancellationToken,
    pub decision: Option<oneshot::Sender<bool>>,
    /// Получатель уже соглашался принять эту передачу.
    pub accepted_once: bool,
    rate: f64,
    rate_at: Instant,
    rate_done: u64,
    last_emit: Instant,
}

impl Transfer {
    fn new(peer: EndpointId, peer_name: String, dir: Dir, what: What) -> Self {
        let id = random_id();
        Transfer {
            wire_id: id.clone(),
            id,
            peer,
            peer_name,
            dir,
            what,
            names: Vec::new(),
            count: 0,
            total: 0,
            done: 0,
            status: Status::Preparing,
            text: None,
            saved_to: None,
            saved_is_dir: false,
            failed_files: 0,
            started: now_unix(),
            finished: 0,
            paths: Vec::new(),
            cancel: CancellationToken::new(),
            decision: None,
            accepted_once: false,
            rate: 0.0,
            rate_at: Instant::now(),
            rate_done: 0,
            last_emit: Instant::now(),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.status.is_finished()
    }

    pub fn is_asking(&self) -> bool {
        self.status == Status::Asking && self.decision.is_some()
    }

    pub fn to_json(&self) -> Value {
        let (text, truncated) = match &self.text {
            Some(t) if t.chars().count() > TEXT_PREVIEW => {
                (Some(t.chars().take(TEXT_PREVIEW).collect::<String>()), true)
            }
            Some(t) => (Some(t.clone()), false),
            None => (None, false),
        };
        json!({
            "id": self.id,
            "peer": self.peer.to_string(),
            "peer_name": self.peer_name,
            "dir": if self.dir == Dir::Out { "out" } else { "in" },
            "kind": if self.what == What::Files { "files" } else { "text" },
            "names": self.names,
            "count": self.count,
            "total": self.total,
            "done": self.done.min(self.total),
            "rate": self.rate.round() as u64,
            "status": self.status.code(),
            "error": match &self.status { Status::Failed(e) => Some(e.clone()), _ => None },
            "by_peer": matches!(self.status, Status::Cancelled { by_peer: true }),
            "text": text,
            "text_truncated": truncated,
            "saved_to": self.saved_to.as_ref().map(|p| p.to_string_lossy().into_owned()),
            "saved_is_dir": self.saved_is_dir,
            "failed_files": self.failed_files,
            "started": self.started,
            "finished": self.finished,
            "can_retry": self.dir == Dir::Out && self.what == What::Files && matches!(
                self.status, Status::Failed(_) | Status::Cancelled { .. }
            ),
        })
    }

    pub fn request_json(&self) -> Value {
        json!({
            "id": self.id,
            "kind": "offer",
            "transfer": self.id,
            "peer": self.peer.to_string(),
            "name": self.peer_name,
            "count": self.count,
            "total": self.total,
            "names": self.names,
        })
    }
}

/// Файл на отправку: где лежит и как его увидит получатель.
#[derive(Clone, Debug)]
pub struct Source {
    pub path: PathBuf,
    pub meta: FileMeta,
}

/// Ошибки отправки: от них зависит, повторять ли попытку.
enum SendErr {
    Cancelled,
    PeerCancelled,
    Declined,
    /// Не удалось связаться — ждём, пока устройство появится.
    Unreachable(anyhow::Error),
    /// Обрыв посреди передачи — повторим с докачкой.
    Net(anyhow::Error),
    /// Повторять бессмысленно.
    Fatal(String),
}

enum RecvErr {
    Cancelled,
    PeerCancelled,
    Net(anyhow::Error),
    Fatal(String),
}

// ---------------------------------------------------------------- узел

impl Node {
    fn push_transfer(&self, t: Transfer) {
        self.with(|s| {
            s.transfers.push(t);
            if s.transfers.len() > MAX_LIST {
                let mut excess = s.transfers.len() - MAX_LIST;
                s.transfers.retain(|t| {
                    if excess > 0 && t.is_finished() {
                        excess -= 1;
                        false
                    } else {
                        true
                    }
                });
            }
        });
        self.events.changed();
    }

    fn update<R>(&self, id: &str, f: impl FnOnce(&mut Transfer) -> R) -> Option<R> {
        self.with(|s| s.transfers.iter_mut().find(|t| t.id == id).map(f))
    }

    fn set_status(&self, id: &str, status: Status) {
        self.update(id, |t| {
            if status.is_finished() {
                t.finished = now_unix();
                t.rate = 0.0;
                t.decision = None;
            }
            t.status = status;
        });
        self.events.changed();
    }

    fn progress(&self, id: &str, n: u64) {
        let emit = self
            .update(id, |t| {
                t.done = t.done.saturating_add(n);
                let now = Instant::now();
                let dt = now.duration_since(t.rate_at).as_secs_f64();
                if dt >= 0.5 {
                    let inst = t.done.saturating_sub(t.rate_done) as f64 / dt;
                    t.rate = if t.rate == 0.0 { inst } else { 0.6 * t.rate + 0.4 * inst };
                    t.rate_at = now;
                    t.rate_done = t.done;
                }
                if now.duration_since(t.last_emit) >= Duration::from_millis(200) {
                    t.last_emit = now;
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if emit {
            self.events.changed();
        }
    }

    // ------------------------------------------------------------ команды

    pub fn send_files(self: &Arc<Self>, peer: EndpointId, paths: Vec<PathBuf>) -> Result<String> {
        if self.kind(&peer).is_none() {
            bail!("not_found");
        }
        if paths.is_empty() {
            bail!("empty");
        }
        let mut t = Transfer::new(peer, self.peer_name(&peer), Dir::Out, What::Files);
        t.names = paths.iter().take(3).map(|p| display_name(p)).collect();
        t.count = paths.len();
        t.paths = paths;
        let id = t.id.clone();
        self.push_transfer(t);
        tokio::spawn(self.clone().run_send(id.clone()));
        Ok(id)
    }

    /// «Повторить»: отправить то же самое ещё раз (получатель докачает).
    pub fn retry_transfer(self: &Arc<Self>, id: &str) -> Result<()> {
        let peer = self
            .update(id, |t| {
                let can = t.dir == Dir::Out
                    && t.what == What::Files
                    && matches!(t.status, Status::Failed(_) | Status::Cancelled { .. });
                if can {
                    t.status = Status::Preparing;
                    t.cancel = CancellationToken::new();
                    t.done = 0;
                    t.failed_files = 0;
                    t.finished = 0;
                    t.started = now_unix();
                    Some(t.peer)
                } else {
                    None
                }
            })
            .flatten();
        let Some(peer) = peer else { bail!("not_found") };
        if self.kind(&peer).is_none() {
            self.set_status(id, Status::Failed("removed".into()));
            bail!("removed");
        }
        self.events.changed();
        tokio::spawn(self.clone().run_send(id.to_string()));
        Ok(())
    }

    pub fn send_text(self: &Arc<Self>, peer: EndpointId, text: String) -> Result<String> {
        if self.kind(&peer).is_none() {
            bail!("not_found");
        }
        if text.trim().is_empty() {
            bail!("empty");
        }
        if text.len() > MAX_TEXT {
            bail!("too_big");
        }
        let mut t = Transfer::new(peer, self.peer_name(&peer), Dir::Out, What::Text);
        t.count = 1;
        t.total = text.len() as u64;
        t.text = Some(text.clone());
        let id = t.id.clone();
        self.push_transfer(t);
        tokio::spawn(self.clone().run_send_text(id.clone(), text));
        Ok(id)
    }

    pub fn respond_offer(&self, id: &str, accept: bool) -> Result<()> {
        match self.update(id, |t| t.decision.take()).flatten() {
            Some(tx) => {
                let _ = tx.send(accept);
                Ok(())
            }
            None => bail!("not_found"),
        }
    }

    pub fn cancel_transfer(&self, id: &str) -> Result<()> {
        let found = self.update(id, |t| {
            if let Some(tx) = t.decision.take() {
                let _ = tx.send(false);
            }
            t.cancel.cancel();
        });
        if found.is_none() {
            bail!("not_found");
        }
        Ok(())
    }

    pub(crate) fn cancel_transfers_with(&self, peer: &EndpointId) {
        self.with(|s| {
            for t in s.transfers.iter_mut().filter(|t| &t.peer == peer && !t.is_finished()) {
                if let Some(tx) = t.decision.take() {
                    let _ = tx.send(false);
                }
                t.cancel.cancel();
            }
        });
    }

    /// Убрать из списка одну завершённую передачу.
    pub fn dismiss_transfer(&self, id: &str) {
        self.with(|s| s.transfers.retain(|t| !(t.id == id && t.is_finished())));
        self.events.changed();
    }

    /// Очистить список от завершённых передач.
    pub fn clear_transfers(&self) {
        self.with(|s| s.transfers.retain(|t| !t.is_finished()));
        self.events.changed();
    }

    pub fn get_text(&self, id: &str) -> Option<String> {
        self.with(|s| s.transfers.iter().find(|t| t.id == id).and_then(|t| t.text.clone()))
    }

    // ------------------------------------------------------------ отправка

    async fn run_send(self: Arc<Self>, id: String) {
        let Some((peer, paths, cancel)) = self.update(&id, |t| (t.peer, t.paths.clone(), t.cancel.clone()))
        else {
            return;
        };
        let files = match tokio::task::spawn_blocking(move || collect(&paths)).await {
            Ok(Ok(f)) if !f.is_empty() => f,
            Ok(Ok(_)) => return self.set_status(&id, Status::Failed("empty".into())),
            Ok(Err(e)) => return self.set_status(&id, Status::Failed(error_code(&e))),
            Err(_) => return self.set_status(&id, Status::Failed("failed".into())),
        };
        let total: u64 = files.iter().map(|f| f.meta.size).sum();
        let names = top_names(&files);
        self.update(&id, |t| {
            t.count = files.len();
            t.total = total;
            t.names = names;
        });
        self.events.changed();

        let started = Instant::now();
        let mut retries = 0;
        loop {
            let res = self.send_once(&id, peer, &files, total, &cancel).await;
            let next = match res {
                Ok(0) => Some(Status::Done),
                Ok(failed) => {
                    self.update(&id, |t| t.failed_files = failed);
                    Some(Status::Failed("verify".into()))
                }
                Err(SendErr::Cancelled) => Some(Status::Cancelled { by_peer: false }),
                Err(SendErr::PeerCancelled) => Some(Status::Cancelled { by_peer: true }),
                Err(SendErr::Declined) => Some(Status::Declined),
                Err(SendErr::Fatal(code)) => Some(Status::Failed(code)),
                Err(SendErr::Unreachable(e)) => {
                    // Устройство не в сети: ждём его (до 10 минут), потом сдаёмся.
                    if started.elapsed() > WAIT_ONLINE || self.kind(&peer).is_none() {
                        self.events.log(format!("send {id}: {e:#}"));
                        Some(Status::Failed("unreachable".into()))
                    } else {
                        self.set_status(&id, Status::WaitingPeer);
                        if !self.wait_online(peer, &cancel, Duration::from_secs(60)).await
                            && cancel.is_cancelled()
                        {
                            Some(Status::Cancelled { by_peer: false })
                        } else {
                            None
                        }
                    }
                }
                Err(SendErr::Net(e)) => {
                    retries += 1;
                    self.events.log(format!("send {id}: {e:#}"));
                    if retries > RETRIES || self.kind(&peer).is_none() {
                        Some(Status::Failed("interrupted".into()))
                    } else {
                        self.set_status(&id, Status::Retrying);
                        if !self.wait_online(peer, &cancel, Duration::from_secs(60)).await
                            && cancel.is_cancelled()
                        {
                            Some(Status::Cancelled { by_peer: false })
                        } else {
                            None
                        }
                    }
                }
            };
            if let Some(status) = next {
                if status == Status::Done {
                    self.update(&id, |t| t.done = t.total);
                }
                self.set_status(&id, status);
                return;
            }
        }
    }

    /// Ждём, пока появится соединение с устройством. false — не дождались.
    async fn wait_online(&self, peer: EndpointId, cancel: &CancellationToken, max: Duration) -> bool {
        let until = Instant::now() + max;
        let mut last_kick = Instant::now() - Duration::from_secs(60);
        while Instant::now() < until {
            if cancel.is_cancelled() {
                return false;
            }
            if self.live_conn(&peer).is_some() {
                return true;
            }
            if last_kick.elapsed() > Duration::from_secs(10) {
                last_kick = Instant::now();
                self.refresh();
            }
            tokio::select! {
                _ = cancel.cancelled() => return false,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
        self.live_conn(&peer).is_some()
    }

    async fn send_once(
        self: &Arc<Self>,
        id: &str,
        peer: EndpointId,
        files: &[Source],
        total: u64,
        cancel: &CancellationToken,
    ) -> Result<usize, SendErr> {
        self.set_status(id, Status::Connecting);
        let conn = tokio::select! {
            _ = cancel.cancelled() => return Err(SendErr::Cancelled),
            r = self.connection_to(peer) => match r {
                Ok(c) => c,
                Err(e) if error_code(&e) == "removed" => return Err(SendErr::Fatal("removed".into())),
                Err(e) => return Err(SendErr::Unreachable(e)),
            },
        };
        self.mark_busy(&conn, true);
        let res = self.send_on(&conn, id, files, total, cancel).await;
        self.mark_busy(&conn, false);
        if let Err(SendErr::Net(_)) = &res
            && let Some(code) = crate::node::close_code(&conn)
            && (code == CODE_REMOVED as u64 || code == CODE_NOT_PAIRED as u64)
        {
            return Err(SendErr::Fatal("removed".into()));
        }
        res
    }

    async fn send_on(
        &self,
        conn: &Connection,
        id: &str,
        files: &[Source],
        total: u64,
        cancel: &CancellationToken,
    ) -> Result<usize, SendErr> {
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| SendErr::Net(anyhow!(e)))?;
        let wire_id = self.update(id, |t| t.wire_id.clone()).unwrap_or_else(|| id.to_string());
        let metas: Vec<FileMeta> = files.iter().map(|f| f.meta.clone()).collect();
        write_msg(&mut send, &Msg::Offer { id: wire_id, files: metas, total }).await.map_err(send_err)?;
        self.set_status(id, Status::Waiting);

        let reply = tokio::select! {
            _ = cancel.cancelled() => {
                abort(&mut send, &mut recv);
                return Err(SendErr::Cancelled);
            }
            r = timeout(Duration::from_secs(15 * 60), read_msg(&mut recv)) => match r {
                Ok(Ok(m)) => m,
                Ok(Err(e)) => return Err(send_err(e)),
                Err(_) => {
                    abort(&mut send, &mut recv);
                    return Err(SendErr::Fatal("timeout".into()));
                }
            },
        };
        let offsets = match reply {
            Msg::OfferAccept { offsets } if offsets.len() == files.len() => offsets,
            Msg::OfferDecline { reason } => {
                return Err(if reason == "declined" { SendErr::Declined } else { SendErr::Fatal(reason) });
            }
            _ => return Err(SendErr::Fatal("protocol".into())),
        };
        let already: u64 = files.iter().zip(&offsets).map(|(f, o)| (*o).min(f.meta.size)).sum();
        self.update(id, |t| {
            t.done = already;
            t.rate = 0.0;
            t.rate_at = Instant::now();
            t.rate_done = already;
            t.status = Status::Active;
        });
        self.events.changed();

        for (i, f) in files.iter().enumerate() {
            let off = offsets[i].min(f.meta.size);
            if off == f.meta.size {
                continue; // у получателя уже есть целиком
            }
            let hash = tokio::select! {
                _ = cancel.cancelled() => {
                    abort(&mut send, &mut recv);
                    return Err(SendErr::Cancelled);
                }
                r = self.send_file(id, &f.path, f.meta.size, off, &mut send) => r?,
            };
            write_msg(&mut send, &Msg::FileHash { index: i as u32, hash }).await.map_err(send_err)?;
        }
        send.finish().map_err(|e| SendErr::Net(anyhow!(e)))?;

        let done = tokio::select! {
            _ = cancel.cancelled() => {
                abort(&mut send, &mut recv);
                return Err(SendErr::Cancelled);
            }
            r = timeout(Duration::from_secs(5 * 60), read_msg(&mut recv)) => match r {
                Ok(Ok(m)) => m,
                Ok(Err(e)) => return Err(send_err(e)),
                Err(_) => return Err(SendErr::Net(anyhow!("timeout"))),
            },
        };
        match done {
            Msg::Complete { failed, .. } => Ok(failed.len()),
            _ => Err(SendErr::Fatal("protocol".into())),
        }
    }

    /// Отправить файл с позиции `off`; вернуть хеш всего файла.
    async fn send_file(
        &self,
        id: &str,
        path: &Path,
        size: u64,
        off: u64,
        send: &mut SendStream,
    ) -> Result<String, SendErr> {
        let mut f = tokio::fs::File::open(path).await.map_err(|_| SendErr::Fatal("read".into()))?;
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; CHUNK];
        let mut broken = false;

        // Начало, которое у получателя уже есть: только считаем хеш.
        let mut left = off;
        while left > 0 {
            let want = left.min(CHUNK as u64) as usize;
            let n = f.read(&mut buf[..want]).await.map_err(|_| SendErr::Fatal("read".into()))?;
            if n == 0 {
                broken = true;
                break;
            }
            hasher.update(&buf[..n]);
            left -= n as u64;
        }

        // Остаток. Если файл укоротился, добиваем нулями до заявленного
        // размера и отправляем заведомо неверный хеш: получатель отбракует
        // только этот файл, остальные дойдут.
        let mut left = size - off;
        while left > 0 {
            let want = left.min(CHUNK as u64) as usize;
            let n = if broken {
                0
            } else {
                f.read(&mut buf[..want]).await.map_err(|_| SendErr::Fatal("read".into()))?
            };
            let n = if n == 0 {
                broken = true;
                buf[..want].fill(0);
                want
            } else {
                n
            };
            send.write_all(&buf[..n]).await.map_err(|e| send_err(anyhow!(e)))?;
            hasher.update(&buf[..n]);
            left -= n as u64;
            self.progress(id, n as u64);
        }
        Ok(if broken { "0".repeat(64) } else { hasher.finalize().to_hex().to_string() })
    }

    async fn run_send_text(self: Arc<Self>, id: String, text: String) {
        let Some((peer, cancel, wire_id)) =
            self.update(&id, |t| (t.peer, t.cancel.clone(), t.wire_id.clone()))
        else {
            return;
        };
        self.set_status(&id, Status::Connecting);
        let job = async {
            let conn = self.connection_to(peer).await?;
            let (mut send, mut recv) = conn.open_bi().await?;
            write_msg(&mut send, &Msg::Text { id: wire_id, text }).await?;
            send.finish()?;
            match timeout(Duration::from_secs(30), read_msg(&mut recv)).await {
                Ok(Ok(Msg::Complete { .. })) => Ok(()),
                Ok(Ok(_)) => bail!("protocol"),
                Ok(Err(e)) => Err(e),
                Err(_) => bail!("timeout"),
            }
        };
        let status = tokio::select! {
            _ = cancel.cancelled() => Status::Cancelled { by_peer: false },
            r = job => match r {
                Ok(()) => Status::Done,
                Err(e) => {
                    self.events.log(format!("text {id}: {e:#}"));
                    Status::Failed(error_code(&e))
                }
            },
        };
        if status == Status::Done {
            self.update(&id, |t| t.done = t.total);
        }
        self.set_status(&id, status);
    }

    // ------------------------------------------------------------ приём

    pub(crate) async fn receive_text(
        &self,
        peer: EndpointId,
        mut send: SendStream,
        wire_id: String,
        text: String,
    ) {
        if text.len() > MAX_TEXT || text.trim().is_empty() {
            let _ = send.reset(VarInt::from_u32(CODE_CANCELLED));
            return;
        }
        let mut t = Transfer::new(peer, self.peer_name(&peer), Dir::In, What::Text);
        t.wire_id = wire_id;
        t.count = 1;
        t.total = text.len() as u64;
        t.done = t.total;
        t.status = Status::Done;
        t.finished = now_unix();
        t.text = Some(text.clone());
        let id = t.id.clone();
        let name = t.peer_name.clone();
        self.push_transfer(t);
        self.events.emit(json!({
            "type": "text", "transfer": id, "peer": peer.to_string(), "name": name,
            "text": text.chars().take(TEXT_PREVIEW).collect::<String>(),
        }));
        if write_msg(&mut send, &Msg::Complete { ok: true, failed: vec![] }).await.is_ok() {
            let _ = send.finish();
            let _ = timeout(Duration::from_secs(5), send.stopped()).await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn receive_offer(
        self: Arc<Self>,
        peer: EndpointId,
        mut send: SendStream,
        mut recv: RecvStream,
        wire_id: String,
        files: Vec<FileMeta>,
        total: u64,
    ) {
        let rels = match plan_offer(&files, total) {
            Ok(r) => r,
            Err(reason) => {
                let _ = write_msg(&mut send, &Msg::OfferDecline { reason: reason.into() }).await;
                let _ = send.finish();
                let _ = timeout(Duration::from_secs(5), send.stopped()).await;
                return;
            }
        };
        let Some(kind) = self.kind(&peer) else {
            let _ = send.reset(VarInt::from_u32(CODE_NOT_PAIRED));
            return;
        };
        let names = top_names_meta(&files);
        let cancel = CancellationToken::new();

        // Повторная отправка той же передачи — продолжаем ту же запись.
        let resumed = self.with(|s| {
            s.transfers.iter_mut().find(|t| t.dir == Dir::In && t.peer == peer && t.wire_id == wire_id).map(
                |t| {
                    t.cancel = cancel.clone();
                    t.count = files.len();
                    t.total = total;
                    t.names = names.clone();
                    t.finished = 0;
                    t.failed_files = 0;
                    (t.id.clone(), t.accepted_once)
                },
            )
        });
        let auto = kind == Kind::Paired || resumed.as_ref().is_some_and(|(_, a)| *a);
        let id = match resumed {
            Some((id, _)) => {
                self.set_status(&id, if auto { Status::Active } else { Status::Asking });
                id
            }
            None => {
                let mut t = Transfer::new(peer, self.peer_name(&peer), Dir::In, What::Files);
                t.wire_id = wire_id;
                t.count = files.len();
                t.total = total;
                t.names = names.clone();
                t.cancel = cancel.clone();
                t.status = if auto { Status::Active } else { Status::Asking };
                let id = t.id.clone();
                self.push_transfer(t);
                id
            }
        };

        if !auto {
            let (tx, rx) = oneshot::channel();
            self.update(&id, |t| t.decision = Some(tx));
            self.events.emit(json!({
                "type": "offer", "transfer": id, "peer": peer.to_string(),
                "name": self.peer_name(&peer), "count": files.len(), "total": total, "names": names,
            }));
            let mut probe = [0u8; 1];
            let accepted = tokio::select! {
                r = timeout(ASK_TTL, rx) => matches!(r, Ok(Ok(true))),
                // Отправитель передумал, пока мы спрашивали.
                _ = recv.read(&mut probe) => {
                    self.set_status(&id, Status::Cancelled { by_peer: true });
                    return;
                }
            };
            self.update(&id, |t| t.decision = None);
            if !accepted {
                let _ = write_msg(&mut send, &Msg::OfferDecline { reason: "declined".into() }).await;
                let _ = send.finish();
                self.set_status(&id, Status::Declined);
                let _ = timeout(Duration::from_secs(5), send.stopped()).await;
                return;
            }
            self.update(&id, |t| t.accepted_once = true);
            self.set_status(&id, Status::Active);
        }

        let dl = self.download_dir();
        let prep = {
            let dl = dl.clone();
            let data_dir = self.cfg.data_dir.clone();
            let files = files.clone();
            tokio::task::spawn_blocking(move || {
                let stage = stage_dir(&stage_root(&dl, &data_dir), &peer, &files);
                prepare_stage(&stage, &dl, &files).map(|offsets| (stage, offsets))
            })
            .await
        };
        let (stage, offsets) = match prep {
            Ok(Ok(r)) => r,
            other => {
                let reason = match &other {
                    Ok(Err(e)) if error_code(e) == "no_space" => "no_space",
                    _ => "storage",
                };
                if let Ok(Err(e)) = other {
                    self.events.log(format!("stage: {e:#}"));
                }
                let _ = write_msg(&mut send, &Msg::OfferDecline { reason: reason.into() }).await;
                let _ = send.finish();
                self.set_status(&id, Status::Failed(reason.into()));
                let _ = timeout(Duration::from_secs(5), send.stopped()).await;
                return;
            }
        };
        if let Err(e) = write_msg(&mut send, &Msg::OfferAccept { offsets: offsets.clone() }).await {
            self.events.log(format!("accept: {e:#}"));
            self.set_status(&id, Status::Failed("interrupted".into()));
            return;
        }
        let already: u64 = offsets.iter().sum();
        self.update(&id, |t| {
            t.done = already;
            t.rate = 0.0;
            t.rate_at = Instant::now();
            t.rate_done = already;
        });
        self.events.changed();

        let mut failed: Vec<u32> = Vec::new();
        let body = async {
            for (i, f) in files.iter().enumerate() {
                let off = offsets[i];
                if off == f.size {
                    continue;
                }
                if !self.receive_file(&id, &mut recv, &stage, i, f.size, off).await? {
                    failed.push(i as u32);
                }
            }
            Ok::<(), RecvErr>(())
        };
        let res = tokio::select! {
            _ = cancel.cancelled() => Err(RecvErr::Cancelled),
            r = body => r,
        };

        match res {
            Ok(()) => {
                let ok = failed.is_empty();
                let failed_n = failed.len();
                if write_msg(&mut send, &Msg::Complete { ok, failed }).await.is_ok() {
                    let _ = send.finish();
                }
                let rels2 = rels.clone();
                let files2 = files.clone();
                let stage2 = stage.clone();
                let dl2 = dl.clone();
                let saved =
                    tokio::task::spawn_blocking(move || finalize(&stage2, &dl2, &files2, &rels2)).await;
                match saved {
                    Ok(Ok(place)) => {
                        let name = self.peer_name(&peer);
                        let (path, is_dir) = match place {
                            Some((p, d)) => (Some(p), d),
                            None => (None, false),
                        };
                        self.update(&id, |t| {
                            t.saved_to = path.clone();
                            t.saved_is_dir = is_dir;
                            t.failed_files = failed_n;
                            t.done = t.total;
                        });
                        self.set_status(
                            &id,
                            if failed_n == 0 { Status::Done } else { Status::Failed("verify".into()) },
                        );
                        self.events.emit(json!({
                            "type": "received", "transfer": id, "peer": peer.to_string(),
                            "name": name, "count": files.len(), "failed": failed_n,
                            "saved_to": path.map(|p| p.to_string_lossy().into_owned()),
                            "saved_is_dir": is_dir,
                        }));
                    }
                    other => {
                        if let Ok(Err(e)) = other {
                            self.events.log(format!("finalize: {e:#}"));
                        }
                        self.set_status(&id, Status::Failed("storage".into()));
                    }
                }
                let _ = timeout(Duration::from_secs(10), send.stopped()).await;
            }
            Err(RecvErr::Cancelled) => {
                abort(&mut send, &mut recv);
                remove_stage(&stage).await;
                self.set_status(&id, Status::Cancelled { by_peer: false });
            }
            Err(RecvErr::PeerCancelled) => {
                remove_stage(&stage).await;
                self.set_status(&id, Status::Cancelled { by_peer: true });
            }
            Err(RecvErr::Net(e)) => {
                // Недокачанное остаётся: повторная отправка продолжит.
                self.events.log(format!("recv {id}: {e:#}"));
                self.set_status(&id, Status::Failed("interrupted".into()));
            }
            Err(RecvErr::Fatal(code)) => {
                abort(&mut send, &mut recv);
                self.set_status(&id, Status::Failed(code));
            }
        }
    }

    /// Принять один файл; true — хеш совпал.
    async fn receive_file(
        &self,
        id: &str,
        recv: &mut RecvStream,
        stage: &Path,
        index: usize,
        size: u64,
        off: u64,
    ) -> Result<bool, RecvErr> {
        let storage = |_| RecvErr::Fatal("storage".into());
        let part = stage.join(format!("{index}.part"));
        let mut f = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&part)
            .await
            .map_err(storage)?;
        let mut hasher = blake3::Hasher::new();
        if off > 0 {
            let mut buf = vec![0u8; CHUNK];
            let mut left = off;
            while left > 0 {
                let want = left.min(CHUNK as u64) as usize;
                let n = f.read(&mut buf[..want]).await.map_err(storage)?;
                if n == 0 {
                    return Err(RecvErr::Fatal("storage".into()));
                }
                hasher.update(&buf[..n]);
                left -= n as u64;
            }
        }
        f.set_len(off).await.map_err(storage)?;
        f.seek(SeekFrom::Start(off)).await.map_err(storage)?;

        let mut left = size - off;
        while left > 0 {
            let want = left.min(CHUNK as u64) as usize;
            let chunk = recv.read_chunk(want).await.map_err(|e| recv_err(anyhow!(e)))?;
            let Some(bytes) = chunk else {
                return Err(RecvErr::Net(anyhow!("finished early")));
            };
            f.write_all(&bytes).await.map_err(storage)?;
            hasher.update(&bytes);
            left = left.saturating_sub(bytes.len() as u64);
            self.progress(id, bytes.len() as u64);
        }
        f.flush().await.map_err(storage)?;
        drop(f);

        let msg = read_msg(recv).await.map_err(recv_err)?;
        let Msg::FileHash { index: got, hash } = msg else {
            return Err(RecvErr::Fatal("protocol".into()));
        };
        if got as usize != index {
            return Err(RecvErr::Fatal("protocol".into()));
        }
        let mine = hasher.finalize().to_hex();
        if ct_eq(mine.as_bytes(), hash.as_bytes()) {
            tokio::fs::rename(&part, stage.join(format!("{index}.ok"))).await.map_err(storage)?;
            Ok(true)
        } else {
            let _ = tokio::fs::remove_file(&part).await;
            Ok(false)
        }
    }
}

// ---------------------------------------------------------------- ошибки

fn abort(send: &mut SendStream, recv: &mut RecvStream) {
    let _ = send.reset(VarInt::from_u32(CODE_CANCELLED));
    let _ = recv.stop(VarInt::from_u32(CODE_CANCELLED));
}

/// Код отмены, если поток закрыт собеседником с CODE_CANCELLED.
fn peer_cancel_code(e: &anyhow::Error) -> bool {
    let cancelled = |code: VarInt| code.into_inner() == CODE_CANCELLED as u64;
    for cause in e.chain() {
        if let Some(WriteError::Stopped(c)) = cause.downcast_ref::<WriteError>() {
            return cancelled(*c);
        }
        if let Some(ReadError::Reset(c)) = cause.downcast_ref::<ReadError>() {
            return cancelled(*c);
        }
        if let Some(ReadExactError::ReadError(ReadError::Reset(c))) = cause.downcast_ref::<ReadExactError>() {
            return cancelled(*c);
        }
    }
    false
}

fn is_protocol_error(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<serde_json::Error>().is_some())
        || e.to_string().contains("message too large")
}

fn removed(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        matches!(
            c.downcast_ref::<ConnectionError>(),
            Some(ConnectionError::ApplicationClosed(a))
                if a.error_code.into_inner() == CODE_REMOVED as u64
                    || a.error_code.into_inner() == CODE_NOT_PAIRED as u64
        )
    })
}

fn send_err(e: anyhow::Error) -> SendErr {
    if peer_cancel_code(&e) {
        SendErr::PeerCancelled
    } else if is_protocol_error(&e) {
        SendErr::Fatal("protocol".into())
    } else if removed(&e) {
        SendErr::Fatal("removed".into())
    } else {
        SendErr::Net(e)
    }
}

fn recv_err(e: anyhow::Error) -> RecvErr {
    if peer_cancel_code(&e) {
        RecvErr::PeerCancelled
    } else if is_protocol_error(&e) {
        RecvErr::Fatal("protocol".into())
    } else {
        RecvErr::Net(e)
    }
}

// ---------------------------------------------------------------- файлы

pub(crate) fn display_name(p: &Path) -> String {
    match p.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => p.to_string_lossy().trim_end_matches(['/', '\\']).to_string(),
    }
}

fn mtime_of(md: &fs::Metadata) -> u64 {
    md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

/// Имя верхнего уровня без повторов: «IMG.jpg», «IMG (2).jpg».
fn unique_top(used: &mut HashSet<String>, name: String) -> String {
    let name = if name.trim().is_empty() { "Files".to_string() } else { name };
    if used.insert(name.to_lowercase()) {
        return name;
    }
    let (stem, ext) = split_ext(&name);
    for i in 2.. {
        let candidate = format!("{stem} ({i}){ext}");
        if used.insert(candidate.to_lowercase()) {
            return candidate;
        }
    }
    unreachable!()
}

fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() && name.len() - i <= 10 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// Собрать список файлов для отправки; папки обходятся целиком.
/// Ссылки на папки не раскрываем — так не уйдём в бесконечный круг.
pub fn collect(paths: &[PathBuf]) -> Result<Vec<Source>> {
    let mut out = Vec::new();
    let mut used = HashSet::new();
    for p in paths {
        let md = fs::metadata(p).map_err(|_| anyhow!("not_found"))?;
        let base = unique_top(&mut used, display_name(p));
        if md.is_dir() {
            walk(p, &base, &mut out)?;
        } else if md.is_file() {
            out.push(Source {
                path: p.clone(),
                meta: FileMeta { path: base, size: md.len(), mtime: mtime_of(&md) },
            });
        }
        if out.len() > MAX_FILES {
            bail!("too_big");
        }
    }
    Ok(out)
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<Source>) -> Result<()> {
    let mut stack = vec![(dir.to_path_buf(), prefix.to_string(), 0usize)];
    while let Some((d, pre, depth)) = stack.pop() {
        if depth > 32 {
            continue;
        }
        let Ok(rd) = fs::read_dir(&d) else { continue };
        let mut entries: Vec<fs::DirEntry> = rd.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        let mut subdirs = Vec::new();
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            let path = e.path();
            let rel = format!("{pre}/{name}");
            if ft.is_dir() {
                subdirs.push((path, rel, depth + 1));
            } else if ft.is_file() || ft.is_symlink() {
                let Ok(md) = fs::metadata(&path) else { continue };
                if !md.is_file() {
                    continue;
                }
                out.push(Source { path, meta: FileMeta { path: rel, size: md.len(), mtime: mtime_of(&md) } });
                if out.len() > MAX_FILES {
                    bail!("too_big");
                }
            }
        }
        // В стек в обратном порядке — обход идёт по алфавиту.
        stack.extend(subdirs.into_iter().rev());
    }
    Ok(())
}

fn top_names(files: &[Source]) -> Vec<String> {
    let mut seen = Vec::new();
    for f in files {
        let top = f.meta.path.split('/').next().unwrap_or("").to_string();
        if !seen.contains(&top) {
            seen.push(top);
            if seen.len() >= 3 {
                break;
            }
        }
    }
    seen
}

fn top_names_meta(files: &[FileMeta]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for f in files {
        let top = f.path.split('/').next().unwrap_or("").to_string();
        let top = sanitize_component(&top).unwrap_or_default();
        if !seen.contains(&top) {
            seen.push(top);
            if seen.len() >= 3 {
                break;
            }
        }
    }
    seen
}

fn is_reserved_windows(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        s if s.len() == 4 && (s.starts_with("COM") || s.starts_with("LPT")) => {
            matches!(s.as_bytes()[3], b'1'..=b'9')
        }
        _ => false,
    }
}

/// Одна часть пути от собеседника: убрать то, что недопустимо в именах
/// файлов Windows, Android и macOS. None — часть пути опасна (`..`).
fn sanitize_component(raw: &str) -> Option<String> {
    if raw.is_empty() || raw == "." || raw == ".." {
        return None;
    }
    let mut c: String =
        raw.chars().map(|ch| if ch.is_control() || "\\:*?\"<>|".contains(ch) { '_' } else { ch }).collect();
    while c.ends_with('.') || c.ends_with(' ') {
        c.pop();
        c.push('_');
    }
    if c.len() > 255 {
        return None;
    }
    if is_reserved_windows(&c) {
        c.insert(0, '_');
    }
    Some(c)
}

pub fn sanitize_rel(path: &str) -> Option<Vec<String>> {
    if path.is_empty() || path.len() > 4096 {
        return None;
    }
    let mut out = Vec::new();
    for raw in path.split('/') {
        out.push(sanitize_component(raw)?);
    }
    if out.len() > 64 {
        return None;
    }
    if out[0].starts_with(STAGE_DIR) {
        out[0].insert(0, '_');
    }
    Some(out)
}

/// Проверить предложение: размеры сходятся, пути безопасны, нет повторов.
fn plan_offer(files: &[FileMeta], total: u64) -> Result<Vec<Vec<String>>, &'static str> {
    if files.is_empty() {
        return Err("empty");
    }
    if files.len() > MAX_FILES {
        return Err("too_big");
    }
    let mut sum: u64 = 0;
    for f in files {
        sum = sum.checked_add(f.size).ok_or("protocol")?;
    }
    if sum != total {
        return Err("protocol");
    }
    let mut rels = Vec::with_capacity(files.len());
    let mut seen = HashSet::new();
    let mut dirs = HashSet::new();
    for f in files {
        let rel = sanitize_rel(&f.path).ok_or("bad_path")?;
        if !seen.insert(rel.join("/")) {
            return Err("bad_path");
        }
        for k in 1..rel.len() {
            dirs.insert(rel[..k].join("/"));
        }
        rels.push(rel);
    }
    if rels.iter().any(|r| dirs.contains(&r.join("/"))) {
        return Err("bad_path");
    }
    Ok(rels)
}

/// Куда принимать недокачанное: рядом с папкой загрузок (тогда перенос на
/// место — простое переименование), а если там нельзя создать служебную
/// папку — в данные приложения.
pub(crate) fn stage_root(download_dir: &Path, data_dir: &Path) -> PathBuf {
    let root = download_dir.join(STAGE_DIR);
    if fs::create_dir_all(&root).is_ok() { root } else { data_dir.join("stage") }
}

/// Запас свободного места сверх размера передачи.
const SPACE_MARGIN: u64 = 64 * 1024 * 1024;

pub(crate) fn stage_dir(root: &Path, peer: &EndpointId, files: &[FileMeta]) -> PathBuf {
    let mut h = blake3::Hasher::new();
    h.update(b"ogneliss-share/stage/1");
    h.update(peer.as_bytes());
    for f in files {
        h.update(f.path.as_bytes());
        h.update(&[0]);
        h.update(&f.size.to_le_bytes());
        h.update(&f.mtime.to_le_bytes());
    }
    let key = h.finalize().to_hex();
    root.join(&key.as_str()[..24])
}

/// Подготовить временную папку и посчитать, что уже есть. Места не хватит
/// на остаток — отказ «no_space» до начала передачи.
fn prepare_stage(stage: &Path, download_dir: &Path, files: &[FileMeta]) -> Result<Vec<u64>> {
    fs::create_dir_all(stage).with_context(|| format!("create {}", stage.display()))?;
    if let Some(root) = stage.parent() {
        // Не показывать недокачанное в галерее Android.
        let nomedia = root.join(".nomedia");
        if !nomedia.exists() {
            let _ = fs::write(nomedia, b"");
        }
    }
    let _ = fs::write(stage.join(".touch"), now_unix().to_string());
    let mut offsets = Vec::with_capacity(files.len());
    for (i, f) in files.iter().enumerate() {
        let ok = stage.join(format!("{i}.ok"));
        if f.size == 0 {
            if !ok.exists() {
                fs::write(&ok, b"")?;
            }
            offsets.push(0);
            continue;
        }
        if let Ok(md) = fs::metadata(&ok) {
            if md.len() == f.size {
                offsets.push(f.size);
                continue;
            }
            let _ = fs::remove_file(&ok);
        }
        let part = stage.join(format!("{i}.part"));
        let off = fs::metadata(&part).map(|m| m.len().min(f.size - 1)).unwrap_or(0);
        offsets.push(off);
    }
    let remaining: u64 = files.iter().zip(&offsets).map(|(f, o)| f.size - o).sum();
    let _ = fs::create_dir_all(download_dir);
    if let Ok(free) = fs4::available_space(stage).or_else(|_| fs4::available_space(download_dir))
        && free < remaining.saturating_add(SPACE_MARGIN)
    {
        bail!("no_space");
    }
    Ok(offsets)
}

/// Свободное имя в папке: «фото.jpg», «фото (1).jpg», «фото (2).jpg»…
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if fs::symlink_metadata(&first).is_err() {
        return first;
    }
    let (stem, ext) = split_ext(name);
    for i in 1..10_000 {
        let p = dir.join(format!("{stem} ({i}){ext}"));
        if fs::symlink_metadata(&p).is_err() {
            return p;
        }
    }
    dir.join(format!("{stem} ({}){ext}", random_id()))
}

fn move_file(src: &Path, dest: &Path) -> Result<()> {
    if fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    fs::copy(src, dest).with_context(|| format!("copy to {}", dest.display()))?;
    let _ = fs::remove_file(src);
    Ok(())
}

fn set_mtime(path: &Path, mtime: u64) {
    if let Ok(f) = fs::File::options().write(true).open(path) {
        let _ = f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(mtime));
    }
}

/// Перенести принятые файлы из временной папки на место.
/// Возвращает, что показать пользователю (файл, папку или папку загрузок)
/// и папка ли это.
fn finalize(
    stage: &Path,
    dl: &Path,
    files: &[FileMeta],
    rels: &[Vec<String>],
) -> Result<Option<(PathBuf, bool)>> {
    fs::create_dir_all(dl)?;
    let mut tops: HashMap<String, PathBuf> = HashMap::new();
    let mut items: Vec<(PathBuf, bool)> = Vec::new();
    for (i, rel) in rels.iter().enumerate() {
        let src = stage.join(format!("{i}.ok"));
        if !src.exists() {
            continue;
        }
        let dest = if rel.len() == 1 {
            let d = unique_path(dl, &rel[0]);
            items.push((d.clone(), false));
            d
        } else {
            let top = match tops.get(&rel[0]) {
                Some(t) => t.clone(),
                None => {
                    let t = unique_path(dl, &rel[0]);
                    fs::create_dir_all(&t)?;
                    tops.insert(rel[0].clone(), t.clone());
                    items.push((t.clone(), true));
                    t
                }
            };
            let mut d = top;
            for c in &rel[1..rel.len() - 1] {
                d.push(c);
            }
            fs::create_dir_all(&d)?;
            unique_path(&d, &rel[rel.len() - 1])
        };
        move_file(&src, &dest)?;
        if files[i].mtime > 0 {
            set_mtime(&dest, files[i].mtime);
        }
    }
    let _ = fs::remove_dir_all(stage);
    Ok(match items.len() {
        0 => None,
        1 => Some(items.remove(0)),
        _ => Some((dl.to_path_buf(), true)),
    })
}

async fn remove_stage(stage: &Path) {
    let stage = stage.to_path_buf();
    let _ = tokio::task::spawn_blocking(move || fs::remove_dir_all(stage)).await;
}

/// Удалить брошенные недокачанные передачи старше недели.
pub fn cleanup_stale(root: &Path) {
    let Ok(rd) = fs::read_dir(root) else { return };
    let now = SystemTime::now();
    for e in rd.filter_map(|e| e.ok()) {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let touched = fs::metadata(path.join(".touch"))
            .or_else(|_| fs::metadata(&path))
            .and_then(|m| m.modified())
            .unwrap_or(now);
        if now.duration_since(touched).unwrap_or_default() > STALE_AFTER {
            let _ = fs::remove_dir_all(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_sanitized() {
        assert_eq!(sanitize_rel("Фото/IMG 1.jpg").unwrap(), vec!["Фото", "IMG 1.jpg"]);
        assert!(sanitize_rel("../etc/passwd").is_none());
        assert!(sanitize_rel("a//b").is_none());
        assert!(sanitize_rel("/abs").is_none());
        assert!(sanitize_rel("a/./b").is_none());
        assert_eq!(sanitize_rel("Встреча 10:30?.txt").unwrap(), vec!["Встреча 10_30_.txt"]);
        assert_eq!(sanitize_rel("con.txt").unwrap(), vec!["_con.txt"]);
        assert_eq!(sanitize_rel("name. ").unwrap(), vec!["name._"]);
        assert_eq!(sanitize_rel(".ogneliss-share/x").unwrap()[0], "_.ogneliss-share");
        assert_eq!(sanitize_rel("a\\b").unwrap(), vec!["a_b"]);
    }

    #[test]
    fn offers_are_validated() {
        let f = |p: &str, s: u64| FileMeta { path: p.into(), size: s, mtime: 0 };
        assert!(plan_offer(&[f("a", 1), f("b", 2)], 3).is_ok());
        assert_eq!(plan_offer(&[f("a", 1)], 2), Err("protocol"));
        assert_eq!(plan_offer(&[f("a", 1), f("a", 1)], 2), Err("bad_path"));
        assert_eq!(plan_offer(&[f("a", 1), f("a/b", 1)], 2), Err("bad_path"));
        assert_eq!(plan_offer(&[f("../x", 1)], 1), Err("bad_path"));
        assert_eq!(plan_offer(&[], 0), Err("empty"));
        assert_eq!(plan_offer(&[f("a", u64::MAX), f("b", 1)], 0), Err("protocol"));
    }

    #[test]
    fn names_are_unique() {
        let mut used = HashSet::new();
        assert_eq!(unique_top(&mut used, "IMG.jpg".into()), "IMG.jpg");
        assert_eq!(unique_top(&mut used, "img.JPG".into()), "img (2).JPG");
        assert_eq!(unique_top(&mut used, "IMG.jpg".into()), "IMG (3).jpg");
        assert_eq!(split_ext(".bashrc"), (".bashrc", ""));
        assert_eq!(split_ext("archive.tar.gz"), ("archive.tar", ".gz"));
    }

    #[test]
    fn no_space_is_detected_before_transfer() {
        let dir = std::env::temp_dir().join(format!("osh-space-{}", random_id()));
        let stage = dir.join(STAGE_DIR).join("x");
        let huge = [FileMeta { path: "big.bin".into(), size: u64::MAX / 2, mtime: 0 }];
        let err = prepare_stage(&stage, &dir, &huge).unwrap_err();
        assert_eq!(crate::node::error_code(&err), "no_space");
        let small = [FileMeta { path: "a.txt".into(), size: 10, mtime: 0 }];
        assert_eq!(prepare_stage(&stage, &dir, &small).unwrap(), vec![0]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn collect_walks_folders() {
        let dir = std::env::temp_dir().join(format!("osh-collect-{}", random_id()));
        fs::create_dir_all(dir.join("Фото/2024")).unwrap();
        fs::write(dir.join("Фото/a.jpg"), b"aa").unwrap();
        fs::write(dir.join("Фото/2024/b.jpg"), b"bbb").unwrap();
        fs::write(dir.join("doc.txt"), b"d").unwrap();
        let list = collect(&[dir.join("Фото"), dir.join("doc.txt")]).unwrap();
        let paths: Vec<&str> = list.iter().map(|s| s.meta.path.as_str()).collect();
        assert_eq!(paths, vec!["Фото/a.jpg", "Фото/2024/b.jpg", "doc.txt"]);
        assert_eq!(list.iter().map(|s| s.meta.size).sum::<u64>(), 6);
        let _ = fs::remove_dir_all(&dir);
    }
}
