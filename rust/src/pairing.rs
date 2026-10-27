//! Ссылка сопряжения (QR-код или сообщение) и проверочный код.
//!
//! Формат: `ogshare://pair?v=1&m=p&id=<ключ>&t=<секрет>&n=<имя>&p=<платформа>&r=<ретранслятор>&a=<адреса>`
//!
//! - `m` — `p` (постоянная связь «Мои устройства»), `c` (постоянная связь
//!   «Чужие устройства»: передачи подтверждаются), `o` (разовый обмен) или
//!   `f` (файлы по ссылке: разовый обмен, файлы приходят сами);
//! - `id` — открытый ключ пригласившего устройства (z-base-32);
//! - `t` — секрет из 16 случайных байт;
//! - `n`, `r`, `a` — имя, адрес ретранслятора и прямые адреса в base64url;
//! - `l=1` — ссылка из сообщения (почта, мессенджер): живёт сутки, прямых
//!   адресов в ней нет, а ответа пригласившего ждём дольше;
//! - `e` — до какого момента действует (секунды Unix), для подсказки.
//!
//! В сообщениях та же строка идёт после `#` в `https://ogneliss.ru/share/join/#…`:
//! такая ссылка кликабельна в любом мессенджере, а часть после `#` браузер
//! на сервер не отправляет — сайт её не видит.
//!
//! Ключ в ссылке защищает от подмены: соединение устанавливается только с
//! владельцем этого ключа (TLS 1.3 с проверкой ключа). Секрет защищает
//! пригласившего: без него к нему нельзя подключиться.

use std::net::SocketAddr;

use anyhow::{Context, Result, bail};
use data_encoding::{BASE32_NOPAD, BASE64URL_NOPAD};
use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};

pub const SCHEME: &str = "ogshare://pair?";
/// Ссылка для сообщений: страница сайта открывает приложение. Отдельный
/// путь /share/join/ — чтобы приложение перехватывало только приглашения,
/// а страницы сайта (описание, политика) открывались в браузере.
pub const WEB_PREFIX: &str = "https://ogneliss.ru/share/join/#";
/// Как узнать ссылку в тексте (после этих меток — параметры).
const MARKERS: &[&str] = &[SCHEME, "ogneliss.ru/share/join/#", "ogneliss.ru/share/join#"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairMode {
    /// Постоянная связь: устройство попадает в «Мои устройства».
    Pair,
    /// Постоянная связь с чужим устройством: «Чужие устройства», каждую
    /// передачу файлов получатель подтверждает.
    Other,
    /// Разовый обмен: временная связь на один сеанс.
    Once,
    /// Файлы по ссылке: разовый обмен, пригласивший сразу отправляет файлы.
    Files,
}

impl PairMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PairMode::Pair => "pair",
            PairMode::Other => "other",
            PairMode::Once => "once",
            PairMode::Files => "files",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pair" | "p" => Some(PairMode::Pair),
            "other" | "c" => Some(PairMode::Other),
            "once" | "o" => Some(PairMode::Once),
            "files" | "f" => Some(PairMode::Files),
            _ => None,
        }
    }

    /// Связь сохраняется (мои или чужие устройства), а не на один сеанс.
    pub fn is_lasting(self) -> bool {
        matches!(self, PairMode::Pair | PairMode::Other)
    }

    fn letter(self) -> &'static str {
        match self {
            PairMode::Pair => "p",
            PairMode::Other => "c",
            PairMode::Once => "o",
            PairMode::Files => "f",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PairLink {
    pub mode: PairMode,
    pub id: EndpointId,
    pub token: [u8; 16],
    pub name: String,
    pub platform: String,
    pub relay: Option<RelayUrl>,
    pub addrs: Vec<SocketAddr>,
    /// Ссылка из сообщения, а не с экрана рядом.
    pub remote: bool,
    /// До какого момента действует (секунды Unix); 0 — не указано.
    pub expires: u64,
}

impl PairLink {
    pub fn token_str(&self) -> String {
        encode_token(&self.token)
    }

    pub fn endpoint_addr(&self) -> EndpointAddr {
        let mut list: Vec<TransportAddr> = Vec::new();
        if let Some(r) = &self.relay {
            list.push(TransportAddr::Relay(r.clone()));
        }
        for a in &self.addrs {
            list.push(TransportAddr::Ip(*a));
        }
        EndpointAddr::from_parts(self.id, list)
    }

    /// Для QR-кода и копирования: `ogshare://pair?…`.
    pub fn to_uri(&self) -> String {
        format!("{SCHEME}{}", self.query())
    }

    /// Для сообщений: `https://ogneliss.ru/share/join/#…`.
    pub fn to_web_uri(&self) -> String {
        format!("{WEB_PREFIX}{}", self.query())
    }

    fn query(&self) -> String {
        let addrs: Vec<String> = self.addrs.iter().map(|a| a.to_string()).collect();
        let mut q = format!(
            "v=1&m={}&id={}&t={}&n={}&p={}",
            self.mode.letter(),
            self.id.to_z32(),
            self.token_str(),
            BASE64URL_NOPAD.encode(self.name.as_bytes()),
            self.platform,
        );
        if let Some(r) = &self.relay {
            q.push_str("&r=");
            q.push_str(&BASE64URL_NOPAD.encode(r.to_string().as_bytes()));
        }
        if !addrs.is_empty() {
            q.push_str("&a=");
            q.push_str(&BASE64URL_NOPAD.encode(addrs.join(",").as_bytes()));
        }
        if self.remote {
            q.push_str("&l=1");
        }
        if self.expires > 0 {
            q.push_str(&format!("&e={}", self.expires));
        }
        q
    }

    /// Найти ссылку в тексте: QR-код, ссылка или сообщение целиком
    /// («Подключитесь ко мне: https://ogneliss.ru/share/join/#…»).
    pub fn parse(text: &str) -> Result<Self> {
        let query = MARKERS
            .iter()
            .filter_map(|m| text.find(m).map(|i| &text[i + m.len()..]))
            .next()
            .context("not an Ogneliss Share link")?;
        // Параметры — до первого пробела или знака, которого в них не бывает.
        let end = query
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | '»'))
            .unwrap_or(query.len());
        let query = &query[..end];
        let mut mode = None;
        let mut id = None;
        let mut token = None;
        let mut name = String::new();
        let mut platform = String::from("other");
        let mut relay = None;
        let mut addrs = Vec::new();
        let mut remote = false;
        let mut expires = 0;
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            match k {
                "v" => {
                    if v != "1" {
                        bail!("unsupported link version");
                    }
                }
                "m" => mode = PairMode::parse(v),
                "id" => id = Some(EndpointId::from_z32(v).context("bad key")?),
                "t" => token = Some(decode_token(v)?),
                "n" => {
                    let raw = BASE64URL_NOPAD.decode(v.as_bytes()).context("bad name")?;
                    name = String::from_utf8_lossy(&raw).into_owned();
                }
                "p" => platform = crate::util::clean_platform(v),
                "r" => {
                    let raw = BASE64URL_NOPAD.decode(v.as_bytes()).context("bad relay")?;
                    let s = String::from_utf8_lossy(&raw).into_owned();
                    relay = s.parse::<RelayUrl>().ok();
                }
                "a" => {
                    let raw = BASE64URL_NOPAD.decode(v.as_bytes()).context("bad addrs")?;
                    let s = String::from_utf8_lossy(&raw).into_owned();
                    addrs =
                        s.split(',').filter_map(|a| a.trim().parse::<SocketAddr>().ok()).take(16).collect();
                }
                "l" => remote = v == "1",
                "e" => expires = v.parse().unwrap_or(0),
                _ => {}
            }
        }
        Ok(PairLink {
            mode: mode.context("no mode")?,
            id: id.context("no key")?,
            token: token.context("no secret")?,
            name: crate::util::clean_name(&name),
            platform,
            relay,
            addrs,
            remote,
            expires,
        })
    }
}

pub fn encode_token(token: &[u8; 16]) -> String {
    BASE32_NOPAD.encode(token).to_lowercase()
}

pub fn decode_token(s: &str) -> Result<[u8; 16]> {
    let raw = BASE32_NOPAD.decode(s.to_uppercase().as_bytes()).context("bad secret")?;
    raw.try_into().map_err(|_| anyhow::anyhow!("bad secret length"))
}

/// Четыре цифры, которые оба экрана показывают при постоянном сопряжении.
/// Совпали — значит, к вам подключается именно то устройство, что у вас в руках.
pub fn pair_code(shower: &EndpointId, scanner: &EndpointId, token: &[u8; 16]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"ogneliss-share/pair-code/1");
    h.update(shower.as_bytes());
    h.update(scanner.as_bytes());
    h.update(token);
    let digest = h.finalize();
    let n = u32::from_le_bytes(digest.as_bytes()[..4].try_into().unwrap_or([0; 4]));
    format!("{:04}", n % 10_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[test]
    fn link_round_trip() {
        let key = SecretKey::generate().public();
        let link = PairLink {
            mode: PairMode::Once,
            id: key,
            token: rand::random(),
            name: "Ноутбук Олега".into(),
            platform: "windows".into(),
            relay: "https://relay.example.org./".parse().ok(),
            addrs: vec!["192.168.1.5:41000".parse().unwrap(), "[fe80::1]:41000".parse().unwrap()],
            remote: false,
            expires: 0,
        };
        let uri = link.to_uri();
        assert!(uri.starts_with(SCHEME));
        let back = PairLink::parse(&uri).unwrap();
        assert_eq!(back.mode, PairMode::Once);
        assert_eq!(back.id, key);
        assert_eq!(back.token, link.token);
        assert_eq!(back.name, "Ноутбук Олега");
        assert_eq!(back.platform, "windows");
        assert_eq!(back.addrs, link.addrs);
        assert_eq!(back.relay, link.relay);
        assert!(!back.remote);
    }

    #[test]
    fn message_links_are_found_in_text() {
        let link = PairLink {
            mode: PairMode::Files,
            id: SecretKey::generate().public(),
            token: rand::random(),
            name: "Телефон".into(),
            platform: "android".into(),
            relay: None,
            addrs: vec![],
            remote: true,
            expires: 1_900_000_000,
        };
        let web = link.to_web_uri();
        assert!(web.starts_with(WEB_PREFIX));
        let message = format!("Файлы для тебя: {web}\nСсылка действует сутки.");
        for text in [web.clone(), message, format!("«{web}»"), web.replace("https://", "")] {
            let back = PairLink::parse(&text).unwrap();
            assert_eq!(back.mode, PairMode::Files);
            assert_eq!(back.id, link.id);
            assert_eq!(back.token, link.token);
            assert!(back.remote);
            assert_eq!(back.expires, 1_900_000_000);
        }
    }

    #[test]
    fn foreign_links_are_rejected() {
        assert!(PairLink::parse("https://example.com").is_err());
        assert!(PairLink::parse("https://example.com/share/#v=1&m=p").is_err());
        assert!(PairLink::parse("https://ogneliss.ru/share/").is_err());
        assert!(PairLink::parse("ogshare://pair?v=2&m=p").is_err());
        assert!(PairLink::parse("ogshare://pair?v=1&m=p&id=zzz&t=aa").is_err());
    }

    #[test]
    fn pair_code_is_stable_and_order_sensitive() {
        let a = SecretKey::generate().public();
        let b = SecretKey::generate().public();
        let t = [7u8; 16];
        let c1 = pair_code(&a, &b, &t);
        assert_eq!(c1, pair_code(&a, &b, &t));
        assert_eq!(c1.len(), 4);
        // Разные роли дают (почти всегда) другой код — роли не путаются.
        let _ = pair_code(&b, &a, &t);
    }
}
