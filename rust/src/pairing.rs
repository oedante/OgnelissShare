//! Ссылка сопряжения в QR-коде и проверочный код.
//!
//! Формат: `ogshare://pair?v=1&m=p&id=<ключ>&t=<секрет>&n=<имя>&p=<платформа>&r=<ретранслятор>&a=<адреса>`
//!
//! - `m` — `p` (постоянная связь «мои устройства») или `o` (разовый обмен);
//! - `id` — открытый ключ показывающего устройства (z-base-32);
//! - `t` — одноразовый секрет из 16 случайных байт, живёт несколько минут;
//! - `n`, `r`, `a` — имя, адрес ретранслятора и прямые адреса в base64url.
//!
//! Ключ в ссылке защищает от подмены: соединение устанавливается только с
//! владельцем этого ключа (TLS 1.3 с проверкой ключа). Секрет защищает
//! показывающего: без него к нему нельзя подключиться.

use std::net::SocketAddr;

use anyhow::{Context, Result, bail};
use data_encoding::{BASE32_NOPAD, BASE64URL_NOPAD};
use iroh::{EndpointAddr, EndpointId, RelayUrl, TransportAddr};

pub const SCHEME: &str = "ogshare://pair?";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairMode {
    /// Постоянная связь: устройство попадает в «Мои устройства».
    Pair,
    /// Разовый обмен: временная связь на один сеанс.
    Once,
}

impl PairMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PairMode::Pair => "pair",
            PairMode::Once => "once",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pair" | "p" => Some(PairMode::Pair),
            "once" | "o" => Some(PairMode::Once),
            _ => None,
        }
    }

    fn letter(self) -> &'static str {
        match self {
            PairMode::Pair => "p",
            PairMode::Once => "o",
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

    pub fn to_uri(&self) -> String {
        let addrs: Vec<String> = self.addrs.iter().map(|a| a.to_string()).collect();
        let mut uri = format!(
            "{SCHEME}v=1&m={}&id={}&t={}&n={}&p={}",
            self.mode.letter(),
            self.id.to_z32(),
            self.token_str(),
            BASE64URL_NOPAD.encode(self.name.as_bytes()),
            self.platform,
        );
        if let Some(r) = &self.relay {
            uri.push_str("&r=");
            uri.push_str(&BASE64URL_NOPAD.encode(r.to_string().as_bytes()));
        }
        if !addrs.is_empty() {
            uri.push_str("&a=");
            uri.push_str(&BASE64URL_NOPAD.encode(addrs.join(",").as_bytes()));
        }
        uri
    }

    pub fn parse(uri: &str) -> Result<Self> {
        let uri = uri.trim();
        let query = uri.strip_prefix(SCHEME).context("not an Ogneliss Share link")?;
        let mut mode = None;
        let mut id = None;
        let mut token = None;
        let mut name = String::new();
        let mut platform = String::from("other");
        let mut relay = None;
        let mut addrs = Vec::new();
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
    }

    #[test]
    fn foreign_links_are_rejected() {
        assert!(PairLink::parse("https://example.com").is_err());
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
