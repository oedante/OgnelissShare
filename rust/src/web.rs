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

use std::collections::HashMap;
use std::io::{self, SeekFrom};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

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
    /// Сколько раз файл забрали целиком (каждый файл считается отдельно).
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
            if let Some(s) = lock(&self.shares).get_mut(&view.token) {
                s.downloads += 1;
            }
            self.events.changed();
        }
        Ok(())
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

// ---------------------------------------------------------------- страницы

const CSS: &str = "\
:root{--p:#1D4F91;--bg:#F2F5F9;--card:#fff;--tx:#14181d;--mu:#5b6673;--on:#fff;color-scheme:light dark}\
@media(prefers-color-scheme:dark){:root{--p:#9DBDEB;--bg:#12161B;--card:#1D232B;--tx:#e8edf3;--mu:#9aa6b4;--on:#12161B}}\
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
    let (from, valid, button, hint) = if ru {
        (
            format!("Файлы с устройства «{device}»"),
            format!("Ссылка действует ещё {mins} мин"),
            "Скачать",
            "Нажмите на каждый файл",
        )
    } else {
        (
            format!("Files from “{device}”"),
            format!("Link valid for {mins} more min"),
            "Download",
            "Tap each file to download",
        )
    };
    let mut items = String::new();
    for (i, f) in view.files.iter().enumerate() {
        items.push_str(&format!(
            "<li><div class=\"n\">{}</div><div class=\"s\">{}</div><a class=\"b\" href=\"f/{i}/{}\">{button}</a></li>",
            escape_html(&f.name),
            human_size(f.size, ru),
            pct_encode(basename(&f.name)),
        ));
    }
    let hint = if view.files.len() > 1 { format!("<p class=\"mu\">{hint}</p>") } else { String::new() };
    layout(ru, &format!("<p class=\"sub\">{from}</p><p class=\"mu\">{valid}</p><ul>{items}</ul>{hint}"))
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
            format!("/s/{}/{tail}", self.info.token)
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
        assert!(html.contains("Скачать") && html.contains("Нажмите на каждый файл"));
        assert!(html.contains("Ссылка действует ещё 10 мин"), "{html}");
        assert!(html.contains("585,9 КБ"));
        assert!(html.contains("href=\"f/0/IMG_1.jpg\""));
        assert!(
            html.contains("href=\"f/2/%D0%94%D0%BE%D0%BA%D1%83%D0%BC%D0%B5%D0%BD%D1%82%20%E2%84%961.txt\"")
        );

        let (_, body) = req(&fx, "GET", &fx.url(""), "Accept-Language: en-US,en;q=0.9\r\n").await;
        let html = String::from_utf8(body).unwrap();
        assert!(html.contains("Download") && html.contains("Tap each file to download"));
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
