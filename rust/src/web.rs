//! Веб-раздача: «получить без приложения».
//!
//! Отправитель временно открывает в локальной сети страницу. Получатель
//! наводит камеру на QR-код, ссылка `http://<IP>:<порт>/s/<токен>/`
//! открывается в любом браузере, и файлы скачиваются прямо с устройства
//! отправителя: без серверов и без JavaScript. Раздача живёт ограниченное
//! время и может быть остановлена раньше.
//!
//! HTTP/1.1 здесь минимальный и написан вручную: только GET и HEAD, одно
//! соединение — один ответ (`Connection: close`), докачка через `Range`.
//! Все файлы раздачи можно забрать одним архивом: ZIP без сжатия собирается
//! на лету прямо в сокет (`all.zip`), его длина известна заранее.

use std::collections::HashMap;
use std::io::{self, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::Context;
use data_encoding::BASE32_NOPAD;
use serde_json::json;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::events::Events;
use crate::util::{ct_eq, now_unix, random_id};

/// Порт по умолчанию; если занят — любой свободный.
const PREFERRED_PORT: u16 = 53318;
/// Сколько соединений обслуживаем одновременно; остальным — 503.
const MAX_CONNS: usize = 32;
/// Предел размера заголовка запроса и время на его получение.
const MAX_HEAD: usize = 16 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(15);
/// Сколько ждём, пока получатель примет очередной кусок данных.
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);
const CHUNK: usize = 256 * 1024;
/// Раздача не живёт дольше суток, что бы ни попросил вызывающий код.
const MAX_TTL: Duration = Duration::from_secs(24 * 3600);

/// Один файл раздачи: где он лежит и как его увидит получатель («Фото/IMG_1.jpg»).
#[derive(Clone, Debug)]
pub struct WebFile {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
}

/// Сводка по раздаче для интерфейса.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ShareInfo {
    pub id: String,
    /// Секрет в ссылке: 16 случайных байт, base32 в нижнем регистре (26 символов).
    pub token: String,
    /// Когда раздача закончится, секунды Unix.
    pub expires: u64,
    pub count: usize,
    pub total: u64,
    /// Первые три имени.
    pub names: Vec<String>,
    /// Сколько раз файл (или весь архив) забрали целиком; каждая отдача считается отдельно.
    pub downloads: u32,
}

struct ShareState {
    id: String,
    /// Порядковый номер: по нему список идёт «сначала новые».
    seq: u64,
    expires: u64,
    deadline: Instant,
    files: Arc<Vec<WebFile>>,
    downloads: u32,
}

impl ShareState {
    fn info(&self, token: &str) -> ShareInfo {
        ShareInfo {
            id: self.id.clone(),
            token: token.to_string(),
            expires: self.expires,
            count: self.files.len(),
            total: self.files.iter().fold(0u64, |a, f| a.saturating_add(f.size)),
            names: self.files.iter().take(3).map(|f| f.name.clone()).collect(),
            downloads: self.downloads,
        }
    }
}

pub struct WebServer {
    port: u16,
    device_name: Mutex<String>,
    /// Токен → раздача. Замок никогда не держим через `.await`.
    shares: Mutex<HashMap<String, ShareState>>,
    seq: AtomicU64,
    conns: AtomicUsize,
    events: Arc<Events>,
}

impl WebServer {
    /// Открыть порт на всех интерфейсах и запустить приём соединений до `cancel`.
    pub async fn bind(
        device_name: String,
        events: Arc<Events>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Arc<WebServer>> {
        let listener = match TcpListener::bind(("0.0.0.0", PREFERRED_PORT)).await {
            Ok(l) => l,
            Err(_) => {
                TcpListener::bind(("0.0.0.0", 0)).await.context("не удалось открыть порт веб-раздачи")?
            }
        };
        let srv = Arc::new(WebServer {
            port: listener.local_addr()?.port(),
            device_name: Mutex::new(device_name),
            shares: Mutex::new(HashMap::new()),
            seq: AtomicU64::new(0),
            conns: AtomicUsize::new(0),
            events,
        });
        tokio::spawn(srv.clone().accept_loop(listener, cancel));
        Ok(srv)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn set_device_name(&self, name: &str) {
        *lock(&self.device_name) = name.to_string();
    }

    /// Новая раздача. Размеры файлов берутся как есть.
    pub fn add(&self, files: Vec<WebFile>, ttl: Duration) -> ShareInfo {
        let ttl = ttl.min(MAX_TTL);
        let token = BASE32_NOPAD.encode(&rand::random::<[u8; 16]>()).to_lowercase();
        let share = ShareState {
            id: random_id(),
            seq: self.seq.fetch_add(1, Ordering::SeqCst),
            expires: now_unix() + ttl.as_secs(),
            deadline: Instant::now() + ttl,
            files: Arc::new(files),
            downloads: 0,
        };
        let info = share.info(&token);
        let mut shares = lock(&self.shares);
        prune(&mut shares);
        shares.insert(token, share);
        info
    }

    /// Остановить раздачу раньше срока. `true`, если она ещё была.
    pub fn remove(&self, id: &str) -> bool {
        let mut shares = lock(&self.shares);
        let before = shares.len();
        shares.retain(|_, s| s.id != id);
        shares.len() != before
    }

    /// Действующие раздачи, сначала новые. Истёкшие убираются здесь же.
    pub fn list(&self) -> Vec<ShareInfo> {
        let mut shares = lock(&self.shares);
        prune(&mut shares);
        let mut all: Vec<(u64, ShareInfo)> = shares.iter().map(|(t, s)| (s.seq, s.info(t))).collect();
        all.sort_by_key(|(seq, _)| std::cmp::Reverse(*seq));
        all.into_iter().map(|(_, i)| i).collect()
    }

    // ------------------------------------------------------------ соединения

    async fn accept_loop(self: Arc<Self>, listener: TcpListener, cancel: CancellationToken) {
        loop {
            let (stream, peer) = tokio::select! {
                _ = cancel.cancelled() => break,
                r = listener.accept() => match r {
                    Ok(x) => x,
                    // Например, кончились дескрипторы: не крутим цикл вхолостую.
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                },
            };
            if self.conns.fetch_add(1, Ordering::SeqCst) >= MAX_CONNS {
                self.conns.fetch_sub(1, Ordering::SeqCst);
                // В пустой буфер нового сокета короткий ответ помещается без ожидания.
                let _ = stream.try_write(head("503 Service Unavailable", &[("Retry-After", "5")]).as_bytes());
                continue;
            }
            let guard = ConnGuard(self.clone());
            let (srv, cancel) = (self.clone(), cancel.clone());
            tokio::spawn(async move {
                let _guard = guard;
                tokio::select! {
                    _ = cancel.cancelled() => {}
                    _ = srv.handle(stream, peer) => {}
                }
            });
        }
    }

    async fn handle(&self, mut stream: TcpStream, peer: SocketAddr) {
        match timeout(HEAD_TIMEOUT, read_head(&mut stream)).await {
            Ok(Ok(raw)) => {
                let _ = match parse_request(&raw) {
                    Some(req) => self.route(&mut stream, peer, &req).await,
                    None => {
                        reply(&mut stream, "400 Bad Request", "text/plain", &[], b"Bad Request", false).await
                    }
                };
            }
            Ok(Err(e)) if e.kind() == io::ErrorKind::InvalidData => {
                let _ =
                    reply(&mut stream, "431 Request Header Fields Too Large", "text/plain", &[], b"", false)
                        .await;
            }
            _ => return,
        }
        // Закрываем аккуратно (FIN), чтобы клиент дочитал всё до конца.
        let _ = stream.shutdown().await;
    }

    async fn route(&self, stream: &mut TcpStream, peer: SocketAddr, req: &Request) -> io::Result<()> {
        let head_only = req.method == "HEAD";
        if req.method != "GET" && !head_only {
            let h = [("Allow", "GET, HEAD")];
            return reply(stream, "405 Method Not Allowed", "text/plain", &h, b"Method Not Allowed", false)
                .await;
        }
        let path = req.target.split('?').next().unwrap_or("");
        let Some(rest) = path.strip_prefix("/s/") else {
            return not_found(stream, req.ru, head_only).await;
        };
        let (token, tail) = match rest.split_once('/') {
            Some((t, tail)) => (t, Some(tail)),
            None => (rest, None),
        };
        let Some(view) = self.find(token) else {
            return not_found(stream, req.ru, head_only).await;
        };
        match tail {
            None => {
                let loc = format!("/s/{}/", view.token);
                reply(stream, "301 Moved Permanently", "text/plain", &[("Location", &loc)], b"", head_only)
                    .await
            }
            Some("") => {
                let html = page(req.ru, &lock(&self.device_name).clone(), &view);
                reply(stream, "200 OK", "text/html; charset=utf-8", &[], html.as_bytes(), head_only).await
            }
            // Архив: `all.zip`, а также `all/<имя>.zip` — чтобы браузер показал красивое имя.
            Some(t) if t == "all.zip" || t.strip_prefix("all/").is_some_and(|r| r.ends_with(".zip")) => {
                self.send_zip(stream, peer, &view, req).await
            }
            Some(t) => {
                let file = t
                    .strip_prefix("f/")
                    .and_then(|s| s.split('/').next())
                    .and_then(|i| i.parse::<usize>().ok())
                    .and_then(|i| view.files.get(i).map(|f| (i, f)));
                match file {
                    Some((i, f)) => self.send_file(stream, peer, &view, i, f, req).await,
                    None => not_found(stream, req.ru, head_only).await,
                }
            }
        }
    }

    /// Раздача по токену из запроса. Сравниваем со всеми токенами без
    /// раннего выхода, чтобы время ответа не выдавало совпавшие символы.
    fn find(&self, token: &str) -> Option<View> {
        let shares = lock(&self.shares);
        let now = Instant::now();
        let mut found = None;
        for (t, s) in shares.iter() {
            if ct_eq(t.as_bytes(), token.as_bytes()) && s.deadline > now {
                found = Some(View {
                    token: t.clone(),
                    id: s.id.clone(),
                    files: s.files.clone(),
                    left: s.deadline - now,
                });
            }
        }
        found
    }

    async fn send_file(
        &self,
        stream: &mut TcpStream,
        peer: SocketAddr,
        view: &View,
        index: usize,
        wf: &WebFile,
        req: &Request,
    ) -> io::Result<()> {
        let head_only = req.method == "HEAD";
        // Размер берём с диска: Content-Length обязан совпасть с тем, что реально отдадим.
        let Ok((mut file, size)) = open_file(&wf.path).await else {
            return not_found(stream, req.ru, head_only).await;
        };
        let (start, len, partial) = match req.range.as_deref().map_or(Range::Whole, |r| parse_range(r, size))
        {
            Range::Whole => (0, size, false),
            Range::Part(s, e) => (s, e - s + 1, true),
            Range::Unsatisfiable => {
                let cr = format!("bytes */{size}");
                return reply(
                    stream,
                    "416 Range Not Satisfiable",
                    "text/plain",
                    &[("Content-Range", &cr)],
                    b"",
                    false,
                )
                .await;
            }
        };

        let base = basename(&wf.name);
        let disposition = format!(
            "attachment; filename=\"{}\"; filename*=UTF-8''{}",
            ascii_fallback(base),
            pct_encode(base)
        );
        let content_length = len.to_string();
        let content_range = format!("bytes {start}-{}/{size}", (start + len).saturating_sub(1));
        let mut headers = vec![
            ("Content-Type", "application/octet-stream"),
            ("Content-Disposition", &disposition),
            ("Content-Length", &content_length),
            ("Accept-Ranges", "bytes"),
            ("Cache-Control", "no-store"),
        ];
        if partial {
            headers.push(("Content-Range", &content_range));
        }
        let status = if partial { "206 Partial Content" } else { "200 OK" };
        put(stream, head(status, &headers).as_bytes()).await?;
        if head_only {
            return Ok(());
        }

        self.events.emit(json!({"type": "web_download", "share": view.id, "name": wf.name}));
        self.events.log(format!("web {}: GET {index} from {}", view.id, peer.ip()));

        if start > 0 {
            file.seek(SeekFrom::Start(start)).await?;
        }
        let mut buf = vec![0u8; CHUNK];
        let mut left = len;
        while left > 0 {
            let want = left.min(CHUNK as u64) as usize;
            let n = file.read(&mut buf[..want]).await?;
            if n == 0 {
                // Файл стал короче во время отдачи: соединение закроется раньше Content-Length.
                return Ok(());
            }
            put(stream, &buf[..n]).await?;
            left -= n as u64;
        }
        // Считаем только отдачу с начала и до конца; закрытие соединения — после подсчёта.
        if start == 0 && len == size {
            self.count_download(&view.token);
        }
        Ok(())
    }

    /// Все файлы раздачи одним ZIP. Размеры и время берём с диска заранее,
    /// поэтому длина архива известна до первого байта (`Content-Length`).
    async fn send_zip(
        &self,
        stream: &mut TcpStream,
        peer: SocketAddr,
        view: &View,
        req: &Request,
    ) -> io::Result<()> {
        let head_only = req.method == "HEAD";
        let mut items = Vec::with_capacity(view.files.len());
        for f in view.files.iter() {
            // Нет одного из файлов — полного архива не будет; неполный молча не отдаём.
            let Ok(meta) = tokio::fs::metadata(&f.path).await else {
                return not_found(stream, req.ru, head_only).await;
            };
            if !meta.is_file() {
                return not_found(stream, req.ru, head_only).await;
            }
            let dos = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(DOS_EPOCH, |d| dos_datetime(d.as_secs()));
            items.push((f.name.clone(), meta.len(), dos));
        }
        let entries = zip_layout(items);
        let title = format!("{}.zip", zip_title(&entries));
        let disposition = format!(
            "attachment; filename=\"{}\"; filename*=UTF-8''{}",
            ascii_fallback(&title),
            pct_encode(&title)
        );
        let content_length = zip_total(&entries).to_string();
        let headers = [
            ("Content-Type", "application/zip"),
            ("Content-Disposition", &disposition),
            ("Content-Length", &content_length),
            ("Accept-Ranges", "none"),
            ("Cache-Control", "no-store"),
        ];
        put(stream, head("200 OK", &headers).as_bytes()).await?;
        if head_only {
            return Ok(());
        }

        self.events.emit(json!({"type": "web_download", "share": view.id, "name": title}));
        self.events.log(format!("web {}: GET zip from {}", view.id, peer.ip()));

        let mut buf = vec![0u8; CHUNK];
        let mut crcs = Vec::with_capacity(entries.len());
        for (e, f) in entries.iter().zip(view.files.iter()) {
            put(stream, &zip_local(e)).await?;
            let mut file = File::open(&f.path).await?;
            let (mut crc, mut left) = (0u32, e.size);
            while left > 0 {
                let want = left.min(CHUNK as u64) as usize;
                let n = file.read(&mut buf[..want]).await?;
                if n == 0 {
                    // Файл стал короче: рвём соединение, а не отдаём испорченный архив.
                    return Err(io::ErrorKind::UnexpectedEof.into());
                }
                crc = crc32_update(crc, &buf[..n]);
                put(stream, &buf[..n]).await?;
                left -= n as u64;
            }
            // Файл не должен стать длиннее заявленного: иначе размер в архиве не совпадёт с данными.
            if file.read(&mut buf[..1]).await? != 0 {
                return Err(io::ErrorKind::InvalidData.into());
            }
            put(stream, &zip_descriptor(e, crc)).await?;
            crcs.push(crc);
        }
        put(stream, &zip_central(&entries, &crcs)).await?;
        self.count_download(&view.token);
        Ok(())
    }

    /// Файл (или архив) отдан целиком: +1 к счётчику и обновить интерфейс.
    fn count_download(&self, token: &str) {
        if let Some(s) = lock(&self.shares).get_mut(token) {
            s.downloads += 1;
        }
        self.events.changed();
    }
}

/// Снимок раздачи для обработки запроса без замка.
struct View {
    token: String,
    id: String,
    files: Arc<Vec<WebFile>>,
    left: Duration,
}

/// Уменьшает счётчик соединений, даже если задача оборвалась.
struct ConnGuard(Arc<WebServer>);

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.conns.fetch_sub(1, Ordering::SeqCst);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn prune(shares: &mut HashMap<String, ShareState>) {
    let now = Instant::now();
    shares.retain(|_, s| s.deadline > now);
}

// ---------------------------------------------------------------- HTTP

struct Request {
    method: String,
    target: String,
    /// Отвечать по-русски (по `Accept-Language`).
    ru: bool,
    range: Option<String>,
}

async fn read_head(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; MAX_HEAD];
    let mut len = 0;
    loop {
        let n = stream.read(&mut buf[len..]).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        len += n;
        if let Some(end) = buf[..len].windows(4).position(|w| w == b"\r\n\r\n") {
            buf.truncate(end);
            return Ok(buf);
        }
        if len == buf.len() {
            return Err(io::ErrorKind::InvalidData.into());
        }
    }
}

fn parse_request(raw: &[u8]) -> Option<Request> {
    let text = String::from_utf8_lossy(raw);
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target, version) = (first.next()?, first.next()?, first.next()?);
    if !version.starts_with("HTTP/") || !target.starts_with('/') {
        return None;
    }
    let (mut ru, mut range) = (false, None);
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        match name.trim().to_ascii_lowercase().as_str() {
            "accept-language" => ru = prefers_russian(value),
            "range" => range = Some(value.trim().to_string()),
            _ => {}
        }
    }
    Some(Request { method: method.to_string(), target: target.to_string(), ru, range })
}

/// Русский, если «ru» встречается в `Accept-Language` раньше «en».
fn prefers_russian(header: &str) -> bool {
    for tag in header.split(',') {
        let lang = tag.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        let primary = lang.split('-').next().unwrap_or("");
        match primary {
            "ru" => return true,
            "en" => return false,
            _ => {}
        }
    }
    false
}

enum Range {
    /// Заголовка нет или он непонятен: отдаём файл целиком.
    Whole,
    /// Включительно, начало и конец внутри файла.
    Part(u64, u64),
    Unsatisfiable,
}

/// Поддерживаем один диапазон `bytes=N-` или `bytes=N-M`; остальное (суффиксы
/// `-N`, несколько диапазонов) по RFC можно проигнорировать.
fn parse_range(header: &str, size: u64) -> Range {
    let Some((a, b)) = header.trim().strip_prefix("bytes=").and_then(|s| s.split_once('-')) else {
        return Range::Whole;
    };
    let Ok(start) = a.trim().parse::<u64>() else { return Range::Whole };
    let end = match b.trim() {
        "" => None,
        e => match e.parse::<u64>() {
            Ok(e) => Some(e),
            Err(_) => return Range::Whole,
        },
    };
    if start >= size {
        return Range::Unsatisfiable;
    }
    let end = end.map_or(size - 1, |e| e.min(size - 1));
    if end < start { Range::Whole } else { Range::Part(start, end) }
}

fn head(status: &str, headers: &[(&str, &str)]) -> String {
    let mut s = format!("HTTP/1.1 {status}\r\n");
    for (k, v) in headers {
        s.push_str(&format!("{k}: {v}\r\n"));
    }
    // Токен сидит в адресе страницы — не отдаём его другим сайтам в Referer.
    s.push_str(
        "Referrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
    );
    s
}

/// Запись с ограничением по времени: зависший получатель не держит соединение вечно.
async fn put(stream: &mut TcpStream, data: &[u8]) -> io::Result<()> {
    timeout(WRITE_TIMEOUT, stream.write_all(data))
        .await
        .unwrap_or_else(|_| Err(io::ErrorKind::TimedOut.into()))
}

/// Короткий ответ целиком. Для HEAD тело не отправляется.
async fn reply(
    stream: &mut TcpStream,
    status: &str,
    ctype: &str,
    extra: &[(&str, &str)],
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
    let len = body.len().to_string();
    let mut headers = vec![("Content-Type", ctype), ("Content-Length", &len), ("Cache-Control", "no-store")];
    headers.extend_from_slice(extra);
    let mut out = head(status, &headers).into_bytes();
    if !head_only {
        out.extend_from_slice(body);
    }
    put(stream, &out).await
}

async fn not_found(stream: &mut TcpStream, ru: bool, head_only: bool) -> io::Result<()> {
    let msg = if ru {
        "Ссылка устарела или неверна. Попросите отправителя показать новый QR-код."
    } else {
        "The link has expired or is wrong. Ask the sender to show a new QR code."
    };
    let html = layout(ru, &format!("<p class=\"sub\">{msg}</p>"));
    reply(stream, "404 Not Found", "text/html; charset=utf-8", &[], html.as_bytes(), head_only).await
}

async fn open_file(path: &Path) -> io::Result<(File, u64)> {
    let file = File::open(path).await?;
    let meta = file.metadata().await?;
    if !meta.is_file() {
        return Err(io::ErrorKind::NotFound.into());
    }
    Ok((file, meta.len()))
}

// ---------------------------------------------------------------- имена

/// Последний сегмент имени: «Фото/IMG_1.jpg» → «IMG_1.jpg».
fn basename(name: &str) -> &str {
    match name.rsplit('/').next() {
        Some(b) if !b.is_empty() => b,
        _ => "file",
    }
}

/// Имя для старых клиентов: только печатный ASCII, без кавычек и обратной косой.
fn ascii_fallback(name: &str) -> String {
    name.chars().map(|c| if matches!(c, ' '..='~') && c != '"' && c != '\\' { c } else { '_' }).collect()
}

/// Процентное кодирование UTF-8: оставляем только буквы, цифры и `-._~`.
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn human_size(n: u64, ru: bool) -> String {
    let units = if ru { ["Б", "КБ", "МБ", "ГБ"] } else { ["B", "KB", "MB", "GB"] };
    if n < 1024 {
        return format!("{n} {}", units[0]);
    }
    let (mut v, mut u) = (n as f64 / 1024.0, 1);
    while v >= 1024.0 && u < 3 {
        v /= 1024.0;
        u += 1;
    }
    let s = format!("{v:.1} {}", units[u]);
    if ru { s.replace('.', ",") } else { s }
}

// ---------------------------------------------------------------- ZIP

// Формат: каждый файл — локальный заголовок, данные без сжатия, дескриптор
// данных; в конце — центральный каталог. CRC32 неизвестен заранее, поэтому он
// идёт в дескрипторе (бит 3), а размеры файлов известны, и длину архива можно
// посчитать до отправки. Всё, что не помещается в 32 бита, пишется в ZIP64.

/// Бит 3: CRC и размеры после данных. Бит 11: имена в UTF-8.
const ZIP_FLAGS: u16 = 0x0808;
/// Значение 0xFFFFFFFF в 32-битном поле означает «смотри ZIP64».
const Z32: u64 = 0xFFFF_FFFF;
/// 1980-01-01 00:00 — когда времени изменения файла нет.
const DOS_EPOCH: (u16, u16) = (0, (1 << 5) | 1);
/// Имя архива, если у файлов нет общей папки верхнего уровня.
const ZIP_DEFAULT_NAME: &str = "Ogneliss Share";

/// Запись чисел в формате ZIP (младший байт первым).
trait PutLe {
    fn u16(&mut self, v: u16);
    fn u32(&mut self, v: u32);
    fn u64(&mut self, v: u64);
}

impl PutLe for Vec<u8> {
    fn u16(&mut self, v: u16) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.extend_from_slice(&v.to_le_bytes());
    }
}

struct ZipEntry {
    /// Имя внутри архива: UTF-8, разделитель «/», без «..» и ведущего «/».
    name: String,
    size: u64,
    dos_time: u16,
    dos_date: u16,
    /// Где в архиве начинается локальный заголовок.
    offset: u64,
}

impl ZipEntry {
    /// Размер не помещается в 32 бита: заголовок и дескриптор в формате ZIP64.
    fn big(&self) -> bool {
        self.size >= Z32
    }

    fn local_len(&self) -> u64 {
        30 + self.name.len() as u64 + if self.big() { 20 } else { 0 }
    }

    fn descriptor_len(&self) -> u64 {
        if self.big() { 24 } else { 16 }
    }

    /// Смещение первого байта после этой записи (вместе с дескриптором).
    fn end(&self) -> u64 {
        self.offset + self.local_len() + self.size + self.descriptor_len()
    }
}

/// Таблица CRC-32 (IEEE, как в ZIP, PNG и gzip), собирается при компиляции.
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

/// Продолжить CRC-32 на следующем куске данных (начинать с 0).
fn crc32_update(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    for &b in data {
        c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// Время ZIP (MS-DOS): `(время, дата)` по UTC; вне 1980–2107 — `DOS_EPOCH`.
fn dos_datetime(unix: u64) -> (u16, u16) {
    // Дата по числу дней с 1970-01-01 (алгоритм Говарда Хиннанта).
    let z = (unix / 86400) as i64 + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if !(1980..=2107).contains(&year) {
        return DOS_EPOCH;
    }
    let secs = (unix % 86400) as i64;
    let time = (secs / 3600) << 11 | (secs % 3600 / 60) << 5 | (secs % 60 / 2);
    (time as u16, ((year - 1980) << 9 | month << 5 | day) as u16)
}

/// Имя файла внутри архива: без ведущего «/», пустых сегментов и «..».
fn zip_name(name: &str) -> String {
    let parts: Vec<&str> = name
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && *p != ".")
        .map(|p| if p == ".." { "_" } else { p })
        .collect();
    let mut s = if parts.is_empty() { "file".to_string() } else { parts.join("/") };
    // Длина имени в заголовке — 16 бит.
    while s.len() > 0xFFFF {
        s.pop();
    }
    s
}

/// Записи архива по списку `(имя, размер, время)` с вычисленными смещениями.
fn zip_layout(items: Vec<(String, u64, (u16, u16))>) -> Vec<ZipEntry> {
    let mut offset = 0;
    items
        .into_iter()
        .map(|(name, size, (dos_time, dos_date))| {
            let e = ZipEntry { name: zip_name(&name), size, dos_time, dos_date, offset };
            offset = e.end();
            e
        })
        .collect()
}

/// Имя архива без «.zip»: общая папка верхнего уровня, иначе «Ogneliss Share».
fn zip_title(entries: &[ZipEntry]) -> String {
    fn top(e: &ZipEntry) -> Option<&str> {
        e.name.split_once('/').map(|(t, _)| t)
    }
    match entries.first().and_then(top) {
        Some(t) if entries.iter().all(|e| top(e) == Some(t)) => t.to_string(),
        _ => ZIP_DEFAULT_NAME.to_string(),
    }
}

/// Локальный заголовок: CRC и размеры нулевые, настоящие придут в дескрипторе.
fn zip_local(e: &ZipEntry) -> Vec<u8> {
    let big = e.big();
    let mut h = Vec::with_capacity(e.local_len() as usize);
    h.u32(0x0403_4B50);
    h.u16(if big { 45 } else { 20 });
    h.u16(ZIP_FLAGS);
    h.u16(0); // без сжатия
    h.u16(e.dos_time);
    h.u16(e.dos_date);
    h.u32(0);
    let sizes = if big { u32::MAX } else { 0 };
    h.u32(sizes);
    h.u32(sizes);
    h.u16(e.name.len() as u16);
    h.u16(if big { 20 } else { 0 });
    h.extend_from_slice(e.name.as_bytes());
    if big {
        // Признак ZIP64: дескриптор будет с 8-байтными размерами.
        h.u16(1);
        h.u16(16);
        h.u64(0);
        h.u64(0);
    }
    h
}

/// Дескриптор данных: CRC и размер сразу после содержимого файла.
fn zip_descriptor(e: &ZipEntry, crc: u32) -> Vec<u8> {
    let mut d = Vec::with_capacity(24);
    d.u32(0x0807_4B50);
    d.u32(crc);
    if e.big() {
        d.u64(e.size);
        d.u64(e.size);
    } else {
        d.u32(e.size as u32);
        d.u32(e.size as u32);
    }
    d
}

/// Центральный каталог и концевые записи (с ZIP64, если что-то не влезло в 32 бита или 16 бит).
fn zip_central(entries: &[ZipEntry], crcs: &[u32]) -> Vec<u8> {
    let mut cd = Vec::new();
    for (e, &crc) in entries.iter().zip(crcs) {
        let (big, far) = (e.big(), e.offset >= Z32);
        let extra_len: u16 =
            if big || far { 4 + if big { 16 } else { 0 } + if far { 8 } else { 0 } } else { 0 };
        let version = if big || far { 45 } else { 20 };
        cd.u32(0x0201_4B50);
        cd.u16(version); // создано в версии
        cd.u16(version); // нужна версия
        cd.u16(ZIP_FLAGS);
        cd.u16(0);
        cd.u16(e.dos_time);
        cd.u16(e.dos_date);
        cd.u32(crc);
        let size32 = if big { u32::MAX } else { e.size as u32 };
        cd.u32(size32);
        cd.u32(size32);
        cd.u16(e.name.len() as u16);
        cd.u16(extra_len);
        cd.u16(0); // комментарий
        cd.u16(0); // номер диска
        cd.u16(0); // внутренние атрибуты
        cd.u32(0); // внешние атрибуты
        cd.u32(if far { u32::MAX } else { e.offset as u32 });
        cd.extend_from_slice(e.name.as_bytes());
        if extra_len > 0 {
            cd.u16(1);
            cd.u16(extra_len - 4);
            if big {
                cd.u64(e.size);
                cd.u64(e.size);
            }
            if far {
                cd.u64(e.offset);
            }
        }
    }
    let count = entries.len() as u64;
    let cd_offset = entries.last().map_or(0, ZipEntry::end);
    let cd_size = cd.len() as u64;
    let zip64 = count >= 0xFFFF || cd_offset >= Z32 || cd_size >= Z32 || entries.iter().any(ZipEntry::big);
    if zip64 {
        // Запись ZIP64 и указатель на неё; обычная концевая запись ниже содержит «заглушки».
        cd.u32(0x0606_4B50);
        cd.u64(44);
        cd.u16(45);
        cd.u16(45);
        cd.u32(0);
        cd.u32(0);
        cd.u64(count);
        cd.u64(count);
        cd.u64(cd_size);
        cd.u64(cd_offset);
        cd.u32(0x0706_4B50);
        cd.u32(0);
        cd.u64(cd_offset + cd_size);
        cd.u32(1);
    }
    cd.u32(0x0605_4B50);
    cd.u16(0);
    cd.u16(0);
    cd.u16(count.min(0xFFFF) as u16);
    cd.u16(count.min(0xFFFF) as u16);
    cd.u32(cd_size.min(u32::MAX.into()) as u32);
    cd.u32(cd_offset.min(u32::MAX.into()) as u32);
    cd.u16(0);
    cd
}

/// Длина архива целиком: записи, центральный каталог и концевые записи.
fn zip_total(entries: &[ZipEntry]) -> u64 {
    entries.last().map_or(0, ZipEntry::end) + zip_central(entries, &vec![0; entries.len()]).len() as u64
}

// ---------------------------------------------------------------- страницы

const CSS: &str = "\
:root{--p:#1D4F91;--bg:#F2F5F9;--card:#fff;--tx:#14181d;--mu:#5b6673;--on:#fff;--tb:#D9E5F6;--tt:#0B2545;color-scheme:light dark}\
@media(prefers-color-scheme:dark){:root{--p:#9DBDEB;--bg:#12161B;--card:#1D232B;--tx:#e8edf3;--mu:#9aa6b4;--on:#12161B;--tb:#233E63;--tt:#D9E5F6}}\
*{box-sizing:border-box}\
body{margin:0;background:var(--bg);color:var(--tx);font:16px/1.45 system-ui,-apple-system,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif}\
main{max-width:560px;margin:0 auto;padding:24px 16px 32px}\
h1{font-size:22px;margin:0 0 16px;color:var(--p)}\
.sub{margin:0 0 4px;font-size:17px;font-weight:600;overflow-wrap:anywhere}\
.mu{margin:0 0 16px;color:var(--mu);font-size:14px}\
ul{list-style:none;margin:0 0 16px;padding:0;display:grid;gap:12px}\
li{background:var(--card);border-radius:16px;padding:16px;box-shadow:0 1px 3px rgba(0,0,0,.1)}\
.n{font-weight:600;overflow-wrap:anywhere}\
.s{margin:2px 0 12px;color:var(--mu);font-size:14px}\
.b{display:block;padding:14px;border-radius:12px;background:var(--p);color:var(--on);font-weight:600;text-align:center;text-decoration:none}\
.t{background:var(--tb);color:var(--tt)}\
.z{margin:0 0 12px;padding:18px 14px;font-size:17px}\
footer{margin-top:24px;color:var(--mu);font-size:13px;text-align:center}\
footer a{color:var(--p)}";

/// Общая рамка страниц: заголовок, стили, подвал.
fn layout(ru: bool, inner: &str) -> String {
    let (lang, foot) = if ru {
        ("ru", "Передаётся напрямую с устройства, без серверов")
    } else {
        ("en", "Sent directly from the device, no servers")
    };
    format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <meta name=\"color-scheme\" content=\"light dark\"><meta name=\"robots\" content=\"noindex\">\
         <title>Ogneliss Share</title><style>{CSS}</style></head><body><main>\
         <h1>Ogneliss Share</h1>{inner}\
         <footer>{foot} · <a href=\"https://ogneliss.ru/share/\" rel=\"noopener noreferrer\">ogneliss.ru/share</a></footer>\
         </main></body></html>"
    )
}

/// Страница со списком файлов раздачи.
fn page(ru: bool, device: &str, view: &View) -> String {
    let device = escape_html(device);
    let mins = view.left.as_secs().div_ceil(60).max(1);
    let total = view.files.iter().fold(0u64, |a, f| a.saturating_add(f.size));
    let (from, valid, button, zip, hint) = if ru {
        (
            format!("Файлы с устройства «{device}»"),
            format!("Ссылка действует ещё {mins} мин"),
            "Скачать",
            format!("Скачать всё одним архивом (ZIP, {})", human_size(total, ru)),
            "Или скачайте файлы по одному",
        )
    } else {
        (
            format!("Files from “{device}”"),
            format!("Link valid for {mins} more min"),
            "Download",
            format!("Download all as one ZIP ({})", human_size(total, ru)),
            "Or download files one by one",
        )
    };
    // При архиве главная кнопка — он, у отдельных файлов кнопки спокойнее.
    let class = if view.files.len() > 1 { "b t" } else { "b" };
    let mut items = String::new();
    for (i, f) in view.files.iter().enumerate() {
        items.push_str(&format!(
            "<li><div class=\"n\">{}</div><div class=\"s\">{}</div><a class=\"{class}\" href=\"f/{i}/{}\">{button}</a></li>",
            escape_html(&f.name),
            human_size(f.size, ru),
            pct_encode(basename(&f.name)),
        ));
    }
    // Архив предлагаем, когда файлов больше одного; кнопки отдельных файлов остаются ниже.
    let all = if view.files.len() > 1 {
        format!("<a class=\"b z\" href=\"all.zip\">{zip}</a><p class=\"mu\">{hint}</p>")
    } else {
        String::new()
    };
    layout(ru, &format!("<p class=\"sub\">{from}</p><p class=\"mu\">{valid}</p>{all}<ul>{items}</ul>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: usize = 600_000;

    struct Fx {
        srv: Arc<WebServer>,
        events: Arc<Events>,
        cancel: CancellationToken,
        dir: PathBuf,
        info: ShareInfo,
        data: Vec<u8>,
    }

    impl Fx {
        fn url(&self, tail: &str) -> String {
            self.url_of(&self.info, tail)
        }

        fn url_of(&self, info: &ShareInfo, tail: &str) -> String {
            format!("/s/{}/{tail}", info.token)
        }
    }

    impl Drop for Fx {
        fn drop(&mut self) {
            self.cancel.cancel();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// Сервер и раздача из трёх файлов: большой (несколько кусков), с `<b>` в имени и с кириллицей.
    async fn setup(ttl: Duration) -> Fx {
        let dir = std::env::temp_dir().join(format!("osh-web-{}", random_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let data: Vec<u8> = (0..SIZE).map(|i| (i % 251) as u8).collect();
        let mut files = Vec::new();
        for (disk, name, bytes) in [
            ("a.bin", "Фото/IMG_1.jpg", data.clone()),
            ("b.txt", "docs/<b>x</b>.txt", b"hello".to_vec()),
            ("c.txt", "Документ №1.txt", "привет".as_bytes().to_vec()),
        ] {
            let path = dir.join(disk);
            std::fs::write(&path, &bytes).unwrap();
            files.push(WebFile { path, name: name.to_string(), size: bytes.len() as u64 });
        }
        let events = Arc::new(Events::default());
        let cancel = CancellationToken::new();
        let srv = WebServer::bind("Пикси <3>".to_string(), events.clone(), cancel.clone()).await.unwrap();
        let info = srv.add(files, ttl);
        Fx { srv, events, cancel, dir, info, data }
    }

    /// Сырой запрос, ответ читаем до закрытия соединения: (заголовок, тело).
    async fn raw(port: u16, request: &str) -> (String, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(request.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        let end = out.windows(4).position(|w| w == b"\r\n\r\n").expect("конец заголовка");
        (String::from_utf8_lossy(&out[..end]).into_owned(), out[end + 4..].to_vec())
    }

    async fn req(fx: &Fx, method: &str, path: &str, extra: &str) -> (String, Vec<u8>) {
        raw(fx.srv.port(), &format!("{method} {path} HTTP/1.1\r\nHost: x\r\n{extra}\r\n")).await
    }

    fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
        head.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn listing_page_is_escaped_and_localized() {
        let fx = setup(Duration::from_secs(600)).await;
        let (head, body) = req(&fx, "GET", &fx.url(""), "Accept-Language: ru-RU,ru;q=0.9,en;q=0.5\r\n").await;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert_eq!(header(&head, "content-type"), Some("text/html; charset=utf-8"));
        assert_eq!(header(&head, "cache-control"), Some("no-store"));
        assert_eq!(header(&head, "connection"), Some("close"));
        let html = String::from_utf8(body).unwrap();
        assert!(html.contains("Фото/IMG_1.jpg") && html.contains("Документ №1.txt"));
        assert!(html.contains("docs/&lt;b&gt;x&lt;/b&gt;.txt") && !html.contains("<b>"));
        assert!(html.contains("Пикси &lt;3&gt;"));
        assert!(html.contains("Скачать") && html.contains("Или скачайте файлы по одному"));
        assert!(
            html.contains("<a class=\"b z\" href=\"all.zip\">Скачать всё одним архивом (ZIP, 586,0 КБ)</a>")
        );
        assert!(html.find("all.zip") < html.find("<ul>"));
        assert!(html.contains("Ссылка действует ещё 10 мин"), "{html}");
        assert!(html.contains("585,9 КБ"));
        assert!(html.contains("href=\"f/0/IMG_1.jpg\""));
        assert!(
            html.contains("href=\"f/2/%D0%94%D0%BE%D0%BA%D1%83%D0%BC%D0%B5%D0%BD%D1%82%20%E2%84%961.txt\"")
        );

        let (_, body) = req(&fx, "GET", &fx.url(""), "Accept-Language: en-US,en;q=0.9\r\n").await;
        let html = String::from_utf8(body).unwrap();
        assert!(html.contains("Download") && html.contains("Or download files one by one"));
        assert!(html.contains("Download all as one ZIP (586.0 KB)"));
        assert!(html.contains("585.9 KB") && html.contains("Link valid for 10 more min"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn full_download_counts_and_notifies() {
        let fx = setup(Duration::from_secs(600)).await;
        let (head, body) = req(&fx, "GET", &fx.url("f/0/IMG_1.jpg"), "").await;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert_eq!(body, fx.data);
        assert_eq!(header(&head, "content-length"), Some(SIZE.to_string().as_str()));
        assert_eq!(header(&head, "content-type"), Some("application/octet-stream"));
        assert_eq!(header(&head, "accept-ranges"), Some("bytes"));
        assert_eq!(
            header(&head, "content-disposition"),
            Some("attachment; filename=\"IMG_1.jpg\"; filename*=UTF-8''IMG_1.jpg")
        );
        assert_eq!(fx.srv.list()[0].downloads, 1);
        let events = fx.events.poll()["events"].clone();
        assert_eq!(events[0]["type"], "web_download");
        assert_eq!(events[0]["share"], fx.info.id.as_str());
        assert_eq!(events[0]["name"], "Фото/IMG_1.jpg");
        assert!(
            fx.events
                .log_lines()
                .iter()
                .any(|l| l.ends_with(&format!("web {}: GET 0 from 127.0.0.1", fx.info.id)))
        );

        // Имя с кириллицей: ASCII-вариант с подчёркиваниями и UTF-8 по RFC 5987; индекс без имени тоже работает.
        let (head, body) = req(&fx, "GET", &fx.url("f/2"), "").await;
        assert_eq!(body, "привет".as_bytes());
        assert_eq!(
            header(&head, "content-disposition"),
            Some(
                "attachment; filename=\"________ _1.txt\"; filename*=UTF-8''%D0%94%D0%BE%D0%BA%D1%83%D0%BC%D0%B5%D0%BD%D1%82%20%E2%84%961.txt"
            )
        );
        assert_eq!(fx.srv.list()[0].downloads, 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn range_requests() {
        let fx = setup(Duration::from_secs(600)).await;
        let (head, body) = req(&fx, "GET", &fx.url("f/0/x"), "Range: bytes=5-\r\n").await;
        assert!(head.starts_with("HTTP/1.1 206"), "{head}");
        assert_eq!(header(&head, "content-range"), Some(format!("bytes 5-{}/{SIZE}", SIZE - 1).as_str()));
        assert_eq!(body, fx.data[5..]);

        let (head, body) = req(&fx, "GET", &fx.url("f/0/x"), "Range: bytes=2-9\r\n").await;
        assert!(head.starts_with("HTTP/1.1 206"), "{head}");
        assert_eq!(header(&head, "content-range"), Some(format!("bytes 2-9/{SIZE}").as_str()));
        assert_eq!(body, fx.data[2..10]);

        let (head, _) = req(&fx, "GET", &fx.url("f/0/x"), &format!("Range: bytes={SIZE}-\r\n")).await;
        assert!(head.starts_with("HTTP/1.1 416"), "{head}");
        assert_eq!(header(&head, "content-range"), Some(format!("bytes */{SIZE}").as_str()));

        // Докачка не считается полной загрузкой, а `bytes=0-` — считается.
        assert_eq!(fx.srv.list()[0].downloads, 0);
        let (head, body) = req(&fx, "GET", &fx.url("f/0/x"), "Range: bytes=0-\r\n").await;
        assert!(head.starts_with("HTTP/1.1 206"), "{head}");
        assert_eq!(body, fx.data);
        assert_eq!(fx.srv.list()[0].downloads, 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn head_has_no_body() {
        let fx = setup(Duration::from_secs(600)).await;
        let (head, body) = req(&fx, "HEAD", &fx.url("f/0/x"), "").await;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert_eq!(header(&head, "content-length"), Some(SIZE.to_string().as_str()));
        assert!(body.is_empty());
        let (head, body) = req(&fx, "HEAD", &fx.url(""), "").await;
        assert!(head.starts_with("HTTP/1.1 200") && body.is_empty());
        assert_eq!(fx.srv.list()[0].downloads, 0);
        assert!(fx.events.poll()["events"].as_array().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_paths_and_methods() {
        let fx = setup(Duration::from_secs(600)).await;
        let wrong = "a".repeat(26);
        for path in [format!("/s/{wrong}/"), format!("/s/{wrong}/f/0"), "/".to_string(), "/s/".to_string()] {
            let (head, body) = req(&fx, "GET", &path, "").await;
            assert!(head.starts_with("HTTP/1.1 404"), "{path}: {head}");
            assert!(String::from_utf8(body).unwrap().contains("expired or is wrong"));
        }
        let (head, body) = req(&fx, "GET", &fx.url("f/9/x"), "Accept-Language: ru\r\n").await;
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        assert!(String::from_utf8(body).unwrap().contains("Ссылка устарела или неверна"));
        let (head, _) = req(&fx, "GET", &fx.url("other"), "").await;
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");

        let (head, _) = req(&fx, "POST", &fx.url(""), "Content-Length: 0\r\n").await;
        assert!(head.starts_with("HTTP/1.1 405") && header(&head, "allow") == Some("GET, HEAD"), "{head}");
        let (head, _) = raw(fx.srv.port(), "мусор\r\n\r\n").await;
        assert!(head.starts_with("HTTP/1.1 400"), "{head}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_file_is_404() {
        let fx = setup(Duration::from_secs(600)).await;
        std::fs::remove_file(fx.dir.join("b.txt")).unwrap();
        let (head, _) = req(&fx, "GET", &fx.url("f/1/x"), "").await;
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn redirect_without_trailing_slash() {
        let fx = setup(Duration::from_secs(600)).await;
        let (head, _) = req(&fx, "GET", &format!("/s/{}?x=1", fx.info.token), "").await;
        assert!(head.starts_with("HTTP/1.1 301"), "{head}");
        assert_eq!(header(&head, "location"), Some(fx.url("").as_str()));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn removed_share_is_gone() {
        let fx = setup(Duration::from_secs(600)).await;
        assert_eq!(fx.srv.list().len(), 1);
        assert!(fx.srv.remove(&fx.info.id));
        assert!(!fx.srv.remove(&fx.info.id));
        assert!(fx.srv.list().is_empty());
        let (head, _) = req(&fx, "GET", &fx.url(""), "").await;
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        let (head, _) = req(&fx, "GET", &fx.url("f/0/x"), "").await;
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn expired_share_is_gone() {
        let fx = setup(Duration::from_millis(100)).await;
        let (head, _) = req(&fx, "GET", &fx.url(""), "").await;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        tokio::time::sleep(Duration::from_millis(250)).await;
        let (head, _) = req(&fx, "GET", &fx.url(""), "").await;
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        let (head, _) = req(&fx, "GET", &fx.url("f/0/x"), "").await;
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        assert!(fx.srv.list().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shares_are_listed_newest_first() {
        let fx = setup(Duration::from_secs(600)).await;
        let file = WebFile { path: fx.dir.join("b.txt"), name: "b.txt".into(), size: 5 };
        let second =
            fx.srv.add(vec![file.clone(), file.clone(), file.clone(), file], Duration::from_secs(900));
        assert_eq!(second.token.len(), 26);
        assert!(second.token.chars().all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c)));
        assert_ne!(second.token, fx.info.token);
        assert_eq!((second.count, second.total, second.names.len()), (4, 20, 3));
        assert_eq!(fx.info.total, SIZE as u64 + 5 + "привет".len() as u64);
        let ids: Vec<String> = fx.srv.list().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, [second.id, fx.info.id.clone()]);
    }

    /// Новая раздача из файлов `(имя на диске, имя для получателя, содержимое)`.
    fn share(fx: &Fx, files: &[(&str, &str, &[u8])]) -> ShareInfo {
        let list = files
            .iter()
            .map(|(disk, name, bytes)| {
                let path = fx.dir.join(disk);
                std::fs::write(&path, bytes).unwrap();
                WebFile { path, name: name.to_string(), size: bytes.len() as u64 }
            })
            .collect();
        fx.srv.add(list, Duration::from_secs(600))
    }

    fn downloads(fx: &Fx, id: &str) -> u32 {
        fx.srv.list().iter().find(|i| i.id == id).unwrap().downloads
    }

    fn le16(b: &[u8], at: usize) -> usize {
        u16::from_le_bytes(b[at..at + 2].try_into().unwrap()) as usize
    }

    fn le32(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    fn le64(b: &[u8], at: usize) -> u64 {
        u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
    }

    /// Разбор архива без ZIP64 по центральному каталогу, со сверкой локальных
    /// заголовков и дескрипторов: `(имя, содержимое, CRC)`.
    fn read_zip(z: &[u8]) -> Vec<(String, Vec<u8>, u32)> {
        let eocd = z.len() - 22;
        assert_eq!(le32(z, eocd), 0x0605_4B50);
        let (count, cd_size, cd_off) =
            (le16(z, eocd + 10), le32(z, eocd + 12) as usize, le32(z, eocd + 16) as usize);
        assert_eq!(cd_off + cd_size, eocd);
        let (mut out, mut p, mut next) = (Vec::new(), cd_off, 0);
        for _ in 0..count {
            assert_eq!(le32(z, p), 0x0201_4B50);
            assert_eq!((le16(z, p + 8), le16(z, p + 10)), (0x0808, 0), "флаги и метод");
            let (crc, size) = (le32(z, p + 16), le32(z, p + 24) as usize);
            assert_eq!(le32(z, p + 20) as usize, size);
            let (name_len, extra_len, comment_len, local) =
                (le16(z, p + 28), le16(z, p + 30), le16(z, p + 32), le32(z, p + 42) as usize);
            let name = String::from_utf8(z[p + 46..p + 46 + name_len].to_vec()).unwrap();
            p += 46 + name_len + extra_len + comment_len;

            assert_eq!(local, next, "записи идут подряд");
            assert_eq!((le32(z, local), le16(z, local + 6)), (0x0403_4B50, 0x0808));
            assert_eq!(le32(z, local + 14), 0, "CRC в локальном заголовке нулевой");
            let local_name = le16(z, local + 26);
            assert_eq!(&z[local + 30..local + 30 + local_name], name.as_bytes());
            let data_at = local + 30 + local_name + le16(z, local + 28);
            let d = data_at + size;
            let descriptor = [le32(z, d), le32(z, d + 4), le32(z, d + 8), le32(z, d + 12)];
            assert_eq!(descriptor, [0x0807_4B50, crc, size as u32, size as u32]);
            next = d + 16;
            out.push((name, z[data_at..d].to_vec(), crc));
        }
        assert_eq!(next, cd_off, "каталог сразу после последней записи");
        out
    }

    /// Проверка архива системным `unzip -t`; если его нет — пропускаем.
    fn unzip_test(zip: &Path) {
        match std::process::Command::new("unzip").arg("-tq").arg(zip).output() {
            Ok(out) => assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => panic!("{e}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn zip_download_is_valid() {
        let fx = setup(Duration::from_secs(600)).await;
        let items: [(&str, &str, &[u8]); 3] = [
            ("z1.bin", "Фото/IMG_1.jpg", &fx.data),
            ("z2.txt", "Документ №1.txt", "привет".as_bytes()),
            ("z3.txt", "Фото/пусто.txt", b""),
        ];
        let info = share(&fx, &items);
        // Range для архива не поддерживается: заголовок игнорируется, отдаётся всё.
        let (head, body) = req(&fx, "GET", &fx.url_of(&info, "all.zip"), "Range: bytes=5-\r\n").await;
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert_eq!(header(&head, "content-type"), Some("application/zip"));
        assert_eq!(header(&head, "accept-ranges"), Some("none"));
        assert_eq!(header(&head, "content-length"), Some(body.len().to_string().as_str()));
        assert_eq!(
            header(&head, "content-disposition"),
            Some("attachment; filename=\"Ogneliss Share.zip\"; filename*=UTF-8''Ogneliss%20Share.zip")
        );

        let files = read_zip(&body);
        assert_eq!(files.len(), 3);
        for ((name, data, crc), (_, want_name, want)) in files.iter().zip(&items) {
            assert_eq!(name, want_name);
            assert_eq!(data, want);
            assert_eq!(*crc, crc32_update(0, want));
        }

        assert_eq!(downloads(&fx, &info.id), 1);
        let events = fx.events.poll()["events"].clone();
        assert_eq!(events[0]["type"], "web_download");
        assert_eq!(events[0]["share"], info.id.as_str());
        assert_eq!(events[0]["name"], "Ogneliss Share.zip");
        let log = format!("web {}: GET zip from 127.0.0.1", info.id);
        assert!(fx.events.log_lines().iter().any(|l| l.ends_with(&log)));

        let path = fx.dir.join("all.zip");
        std::fs::write(&path, &body).unwrap();
        unzip_test(&path);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn zip_name_head_and_page_button() {
        let fx = setup(Duration::from_secs(600)).await;
        // Общая папка → имя архива; «..» и ведущий «/» в именах обезврежены.
        let info = share(&fx, &[("p.txt", "Фото/a.txt", b"a"), ("q.txt", "/Фото/../etc/b.txt", b"bb")]);
        let (head, body) = req(&fx, "HEAD", &fx.url_of(&info, "all/%D0%BB.zip"), "").await;
        assert!(head.starts_with("HTTP/1.1 200") && body.is_empty(), "{head}");
        assert_eq!(
            header(&head, "content-disposition"),
            Some("attachment; filename=\"____.zip\"; filename*=UTF-8''%D0%A4%D0%BE%D1%82%D0%BE.zip")
        );
        assert_eq!(downloads(&fx, &info.id), 0);
        assert!(fx.events.poll()["events"].as_array().unwrap().is_empty());

        let (head2, body) = req(&fx, "GET", &fx.url_of(&info, "all/x.zip"), "").await;
        assert_eq!(header(&head, "content-length"), header(&head2, "content-length"));
        assert_eq!(header(&head2, "content-length"), Some(body.len().to_string().as_str()));
        let names: Vec<String> = read_zip(&body).into_iter().map(|f| f.0).collect();
        assert_eq!(names, ["Фото/a.txt", "Фото/_/etc/b.txt"]);

        // Одному файлу архив не нужен: кнопки на странице нет.
        let one = share(&fx, &[("s.txt", "one.txt", b"x")]);
        let (_, body) = req(&fx, "GET", &fx.url_of(&one, ""), "").await;
        assert!(!String::from_utf8(body).unwrap().contains("all.zip"));

        // Недоступный файл — архива нет, а не неполный архив.
        std::fs::remove_file(fx.dir.join("q.txt")).unwrap();
        let (head, _) = req(&fx, "GET", &fx.url_of(&info, "all.zip"), "").await;
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
    }

    /// Файл меняется посреди отдачи архива: соединение рвётся, архив не доходит до конца.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn zip_aborts_when_file_changes() {
        const BIG: u64 = 40 << 20;
        for (case, change) in [("короче", true), ("длиннее", false)] {
            let fx = setup(Duration::from_secs(600)).await;
            let path = fx.dir.join("big.bin");
            std::fs::File::create(&path).unwrap().set_len(BIG).unwrap();
            let info = fx.srv.add(
                vec![WebFile { path: path.clone(), name: "big.bin".into(), size: BIG }],
                Duration::from_secs(600),
            );
            let mut s = TcpStream::connect(("127.0.0.1", fx.srv.port())).await.unwrap();
            let request = format!("GET {} HTTP/1.1\r\nHost: x\r\n\r\n", fx.url_of(&info, "all.zip"));
            s.write_all(request.as_bytes()).await.unwrap();
            // Читаем самое начало: остальное не влезает в буферы сокета, сервер ждёт нас.
            let mut out = vec![0u8; 4096];
            let n = s.read(&mut out).await.unwrap();
            out.truncate(n);
            if change {
                std::fs::OpenOptions::new().write(true).truncate(true).open(&path).unwrap();
            } else {
                use std::io::Write;
                std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"+").unwrap();
            }
            s.read_to_end(&mut out).await.unwrap();
            let end = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
            let head = String::from_utf8_lossy(&out[..end]).into_owned();
            let want: usize = header(&head, "content-length").unwrap().parse().unwrap();
            assert!(out.len() - end - 4 < want, "{case}: архив оборван, а не дописан до конца");
            assert_eq!(downloads(&fx, &info.id), 0, "{case}");
        }
    }

    #[test]
    fn zip_primitives() {
        // Эталон CRC-32 («check value») и продолжение по кускам.
        assert_eq!(crc32_update(0, b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32_update(crc32_update(0, b"1234"), b"56789"), 0xCBF4_3926);
        assert_eq!(crc32_update(0, b""), 0);

        // 2023-11-14 22:13:20 и високосный день 2024-02-29; до 1980 года — запасная дата.
        assert_eq!(dos_datetime(1_700_000_000), ((22 << 11) | (13 << 5) | 10, (43 << 9) | (11 << 5) | 14));
        assert_eq!(dos_datetime(1_709_164_800), (0, (44 << 9) | (2 << 5) | 29));
        assert_eq!(dos_datetime(0), DOS_EPOCH);

        assert_eq!(zip_name("Фото/IMG_1.jpg"), "Фото/IMG_1.jpg");
        assert_eq!(zip_name("/a//b/./c.txt"), "a/b/c.txt");
        assert_eq!(zip_name("../../x\\..\\y"), "_/_/x/_/y");
        assert_eq!(zip_name("///"), "file");
        assert_eq!(zip_name(&"я".repeat(40_000)).len(), 0xFFFE);
    }

    /// Структуры ZIP64 на «выдуманных» размерах: настоящие файлы по 4 ГиБ в тесте не нужны.
    #[test]
    fn zip64_structures() {
        let gib = 1u64 << 30;
        let items = [10, 5 * gib, 10, 0xFFFF_FFFF].map(|s| ("f".to_string(), s, DOS_EPOCH)).to_vec();
        let e = zip_layout(items);
        assert!(!e[0].big() && e[1].big() && !e[2].big() && e[3].big(), "0xFFFFFFFF уже ZIP64");
        assert!(e[1].offset < Z32 && e[2].offset >= Z32 && e[3].offset >= Z32);
        // Локальный заголовок и дескриптор больших записей: ZIP64.
        assert_eq!((zip_local(&e[0]).len(), zip_local(&e[1]).len()), (31, 51));
        assert_eq!((zip_descriptor(&e[0], 7).len(), zip_descriptor(&e[1], 7).len()), (16, 24));

        let central = zip_central(&e, &[1, 2, 3, 4]);
        assert_eq!(zip_total(&e), e[3].end() + central.len() as u64);
        // Дополнительные поля: ничего / размеры / смещение / размеры и смещение.
        let (mut p, mut extras) = (0, Vec::new());
        for entry in &e {
            assert_eq!(le32(&central, p), 0x0201_4B50);
            let (name_len, extra_len) = (le16(&central, p + 28), le16(&central, p + 30));
            extras.push(extra_len);
            if extra_len > 0 {
                let x = p + 46 + name_len;
                assert_eq!((le16(&central, x), le16(&central, x + 2)), (1, extra_len - 4));
                // Смещение — всегда последнее поле дополнения.
                if entry.offset >= Z32 {
                    assert_eq!(le64(&central, x + extra_len - 8), entry.offset);
                }
                if entry.big() {
                    assert_eq!((le64(&central, x + 4), le64(&central, x + 12)), (entry.size, entry.size));
                }
            }
            p += 46 + name_len + extra_len;
        }
        assert_eq!(extras, [0, 20, 12, 28]);

        // Концевые записи: ZIP64 (56 байт), указатель на неё (20) и обычная (22) с заглушками.
        let (cd_size, n) = (p as u64, central.len());
        assert_eq!(n, p + 56 + 20 + 22);
        let (z64, loc, end) = (&central[p..], &central[p + 56..], &central[p + 76..]);
        assert_eq!((le32(z64, 0), le64(z64, 4)), (0x0606_4B50, 44));
        assert_eq!((le64(z64, 24), le64(z64, 32)), (4, 4));
        assert_eq!((le64(z64, 40), le64(z64, 48)), (cd_size, e[3].end()));
        assert_eq!((le32(loc, 0), le64(loc, 8), le32(loc, 16)), (0x0706_4B50, e[3].end() + cd_size, 1));
        assert_eq!(
            (le32(end, 0), le16(end, 10), le32(end, 12), le32(end, 16)),
            (0x0605_4B50, 4, p as u32, u32::MAX)
        );
    }

    /// Больше 65535 записей: счётчик в обычной концевой записи — заглушка, число в ZIP64.
    #[test]
    fn zip_many_entries() {
        let items = (0..70_000).map(|i| (format!("d/{i}.txt"), 0, DOS_EPOCH)).collect();
        let e = zip_layout(items);
        let mut z = Vec::new();
        for x in &e {
            z.extend(zip_local(x));
            z.extend(zip_descriptor(x, 0));
        }
        z.extend(zip_central(&e, &vec![0; e.len()]));
        assert_eq!(z.len() as u64, zip_total(&e));
        let end = z.len() - 22;
        assert_eq!((le16(&z, end + 8), le16(&z, end + 10)), (0xFFFF, 0xFFFF));
        assert_eq!((le64(&z, end - 20 - 56 + 24), le64(&z, end - 20 - 56 + 32)), (70_000, 70_000));

        let path = std::env::temp_dir().join(format!("osh-zip-{}.zip", random_id()));
        std::fs::write(&path, &z).unwrap();
        unzip_test(&path);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn helpers() {
        assert!(prefers_russian("ru-RU,ru;q=0.9,en;q=0.8"));
        assert!(prefers_russian("de;q=0.9, ru;q=0.5, en;q=0.1"));
        assert!(!prefers_russian("en-US,en;q=0.9,ru;q=0.5"));
        assert!(!prefers_russian("") && !prefers_russian("de-DE"));

        assert!(matches!(parse_range("bytes=5-", 10), Range::Part(5, 9)));
        assert!(matches!(parse_range("bytes=2-99", 10), Range::Part(2, 9)));
        assert!(matches!(parse_range("bytes=10-", 10), Range::Unsatisfiable));
        assert!(matches!(parse_range("bytes=0-", 0), Range::Unsatisfiable));
        for ignored in ["bytes=-5", "bytes=0-1,3-4", "items=0-1", "bytes=7-3", "ерунда"] {
            assert!(matches!(parse_range(ignored, 10), Range::Whole), "{ignored}");
        }

        assert_eq!(basename("Фото/IMG_1.jpg"), "IMG_1.jpg");
        assert_eq!(basename("dir/"), "file");
        assert_eq!(ascii_fallback("a\"b\\c\r\nй.txt"), "a_b_c___.txt");
        assert_eq!(pct_encode("a b&é"), "a%20b%26%C3%A9");
        assert_eq!(escape_html("<a href=\"x\">&'"), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
        assert_eq!(human_size(1023, false), "1023 B");
        assert_eq!(human_size(1536, false), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024, true), "5,0 ГБ");
    }
}
