//! «Без приложения»: файлы отдаются в браузер.
//!
//! Рядом — `web::WebServer` в локальной сети: порт открыт, пока есть
//! раздачи, адрес для QR-кода — частный IPv4 устройства (домашняя Wi-Fi,
//! точка доступа телефона), серверов нет. Далеко — `rtc::Room`: ссылка
//! https://ogneliss.ru/share/get/#…, служба связи только сводит стороны,
//! файлы идут напрямую по WebRTC (docs/WEB_P2P.md).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::node::{Node, take_into};
use crate::rtc::Room;
use crate::transfer::collect;
use crate::util::random_id;
use crate::web::{ShareInfo, WebFile, WebServer};

/// Сервер раздач и то, что ему принадлежит.
pub(crate) struct WebSlot {
    server: Arc<WebServer>,
    cancel: CancellationToken,
    /// Забранные копии (с телефона): id раздачи → папка, удалить по окончании.
    dirs: HashMap<String, PathBuf>,
    /// Ссылки через интернет: id раздачи → комната на службе связи.
    rooms: HashMap<String, Arc<Room>>,
    /// Сколько раздач было при прошлой проверке — чтобы сообщить интерфейсу.
    seen: usize,
}

const MAX_SHARES: usize = 8;

impl Node {
    /// Новая раздача: файлы и папки по путям, срок в минутах (5–120).
    /// `take` — пути временные (копии с телефона): узел забирает их себе.
    pub async fn web_share(self: &Arc<Self>, paths: Vec<PathBuf>, minutes: u64, take: bool) -> Result<Value> {
        if paths.is_empty() {
            bail!("empty");
        }
        if paths.iter().any(|p| !p.exists()) {
            bail!("read");
        }
        if self.with(|s| s.web.as_ref().map_or(0, |w| w.server.list().len())) >= MAX_SHARES {
            bail!("too_many");
        }
        let ttl = Duration::from_secs(minutes.clamp(5, 120) * 60);
        let dir = take.then(|| self.cfg.data_dir.join("web").join(random_id()));
        let into = dir.clone();
        let sources = tokio::task::spawn_blocking(move || -> Result<_> {
            match into {
                Some(d) => collect(&take_into(&d, &paths).context("storage")?),
                None => collect(&paths),
            }
        })
        .await
        .map_err(|e| anyhow!("join: {e}"))??;
        if sources.is_empty() {
            remove_dirs(dir.into_iter().collect());
            bail!("empty");
        }
        let server = self.web_server().await?;
        let files: Vec<WebFile> = sources
            .into_iter()
            .map(|s| WebFile { path: s.path, name: s.meta.path, size: s.meta.size })
            .collect();
        let room_files = files.clone();
        let info = server.add(files, ttl);
        let room = self.cfg.rtc.clone().map(|cfg| {
            let ep = self.ep.clone();
            let ips: Arc<dyn Fn() -> Vec<IpAddr> + Send + Sync> = Arc::new(move || host_ips(&ep));
            Room::start(
                cfg,
                info.id.clone(),
                self.name(),
                room_files,
                ttl,
                ips,
                self.events.clone(),
                self.cancel.clone(),
            )
        });
        let json_room = room.clone();
        self.with(|s| {
            if let Some(w) = s.web.as_mut() {
                w.seen += 1;
                if let Some(d) = dir {
                    w.dirs.insert(info.id.clone(), d);
                }
                if let Some(r) = room {
                    w.rooms.insert(info.id.clone(), r);
                }
            }
        });
        self.events.log(format!("web {}: {} files, port {}", info.id, info.count, server.port()));
        self.events.changed();
        Ok(share_json(&info, server.port(), &self.lan_ips(), json_room.as_deref()))
    }

    /// Остановить раздачу досрочно.
    pub fn web_unshare(&self, id: &str) -> Result<()> {
        let found = self.with(|s| s.web.as_ref().is_some_and(|w| w.server.remove(id)));
        if !found {
            bail!("not_found");
        }
        self.web_tidy();
        self.events.changed();
        Ok(())
    }

    /// Раздачи для снимка состояния.
    pub(crate) fn web_snapshot(&self) -> Value {
        let Some(port) = self.with(|s| s.web.as_ref().map(|w| w.server.port())) else {
            return json!([]);
        };
        let ips = self.lan_ips();
        let list = self.web_tidy();
        let rooms = self.with(|s| s.web.as_ref().map(|w| w.rooms.clone()).unwrap_or_default());
        Value::Array(
            list.iter().map(|i| share_json(i, port, &ips, rooms.get(&i.id).map(|r| r.as_ref()))).collect(),
        )
    }

    /// Раз в несколько секунд: истёкшие раздачи убрать, интерфейсу сообщить.
    pub(crate) fn web_tick(&self) {
        if self.with(|s| s.web.is_none()) {
            return;
        }
        let before = self.with(|s| s.web.as_ref().map_or(0, |w| w.seen));
        if self.web_tidy().len() != before {
            self.events.changed();
        }
    }

    pub(crate) fn web_set_name(&self, name: &str) {
        let Some((server, rooms)) = self.with(|s| {
            s.web.as_ref().map(|w| (w.server.clone(), w.rooms.values().cloned().collect::<Vec<_>>()))
        }) else {
            return;
        };
        server.set_device_name(name);
        rooms.iter().for_each(|r| r.set_device_name(name));
    }

    async fn web_server(&self) -> Result<Arc<WebServer>> {
        if let Some(server) = self.with(|s| s.web.as_ref().map(|w| w.server.clone())) {
            return Ok(server);
        }
        let cancel = self.cancel.child_token();
        let server =
            WebServer::bind(self.name(), self.events.clone(), cancel.clone()).await.context("web")?;
        self.events.log(format!("web: listening on port {}", server.port()));
        // Пока сервер поднимался, его мог поднять и другой вызов — оставляем один.
        Ok(self.with(|s| match &s.web {
            Some(w) => {
                cancel.cancel();
                w.server.clone()
            }
            None => {
                s.web = Some(WebSlot {
                    server: server.clone(),
                    cancel,
                    dirs: HashMap::new(),
                    rooms: HashMap::new(),
                    seen: 0,
                });
                server
            }
        }))
    }

    /// Действующие раздачи; копии закончившихся — удалить, нет раздач —
    /// закрыть порт.
    fn web_tidy(&self) -> Vec<ShareInfo> {
        let (list, gone, stopped) = self.with(|s| {
            let Some(w) = s.web.as_mut() else { return (Vec::new(), Vec::new(), false) };
            let list = w.server.list();
            let ended: Vec<String> =
                w.dirs.keys().filter(|id| !list.iter().any(|i| &i.id == *id)).cloned().collect();
            let gone: Vec<PathBuf> = ended.iter().filter_map(|id| w.dirs.remove(id)).collect();
            // Ссылки через интернет закончившихся раздач — закрыть (и комнату на службе).
            w.rooms.retain(|id, room| {
                let keep = list.iter().any(|i| &i.id == id);
                if !keep {
                    room.close();
                }
                keep
            });
            w.seen = list.len();
            let stopped = list.is_empty();
            if stopped {
                w.cancel.cancel();
                s.web = None;
            }
            (list, gone, stopped)
        });
        if stopped {
            self.events.log("web: stopped");
        }
        remove_dirs(gone);
        list
    }

    /// Адреса этого устройства в локальной сети, лучший первым.
    fn lan_ips(&self) -> Vec<Ipv4Addr> {
        let mut ips: Vec<Ipv4Addr> = self
            .ep
            .addr()
            .ip_addrs()
            .filter_map(|a| match a.ip() {
                IpAddr::V4(ip) if lan_rank(ip).is_some() => Some(ip),
                _ => None,
            })
            .collect();
        ips.sort_by_key(|ip| (lan_rank(*ip), *ip));
        ips.dedup();
        ips
    }
}

/// Насколько адрес похож на домашнюю сеть: 192.168.* — Wi-Fi и точки
/// доступа, 172.16–31.* — реже, 10.* — часто VPN, поэтому последним.
/// Публичные адреса, 100.64/10 (операторы, Tailscale) и прочие — не годятся.
fn lan_rank(ip: Ipv4Addr) -> Option<u8> {
    let o = ip.octets();
    match o {
        [192, 168, ..] => Some(0),
        [172, b, ..] if (16..32).contains(&b) => Some(1),
        [10, ..] => Some(2),
        _ => None,
    }
}

fn share_json(i: &ShareInfo, port: u16, ips: &[Ipv4Addr], room: Option<&Room>) -> Value {
    let urls: Vec<String> = ips.iter().map(|ip| format!("http://{ip}:{port}/s/{}/", i.token)).collect();
    json!({
        "id": i.id,
        "url": urls.first().cloned().unwrap_or_default(),
        "urls": urls,
        "link": room.map(|r| r.link()).unwrap_or_default(),
        "link_status": room.map(|r| r.status()).unwrap_or_default(),
        "peers": room.map_or(0, |r| r.peers()),
        "expires": i.expires,
        "count": i.count,
        "total": i.total,
        "names": i.names,
        "downloads": i.downloads + room.map_or(0, |r| r.downloads()),
    })
}

/// Адреса интерфейсов для WebRTC: свои IPv4 и IPv6, кроме петли и
/// локальных для канала (169.254/16, fe80::/10); частные — первыми.
/// Публичный адрес снаружи NAT сюда не попадёт: к нему сокет не
/// привязать, его узнаёт сам rtc через STUN.
fn host_ips(ep: &iroh::Endpoint) -> Vec<IpAddr> {
    let mut ips: Vec<IpAddr> = ep
        .addr()
        .ip_addrs()
        .map(|a| a.ip())
        .filter(|ip| match ip {
            IpAddr::V4(v4) => !(v4.is_loopback() || v4.is_link_local() || v4.is_unspecified()),
            IpAddr::V6(v6) => {
                !(v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xffc0) == 0xfe80)
            }
        })
        .collect();
    ips.sort_by_key(|ip| match ip {
        IpAddr::V4(v4) => (lan_rank(*v4).unwrap_or(3), *ip),
        IpAddr::V6(_) => (4, *ip),
    });
    ips.dedup();
    ips
}

fn remove_dirs(dirs: Vec<PathBuf>) {
    for d in dirs {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_networks_first_vpn_last() {
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        assert_eq!(lan_rank(ip("192.168.1.5")), Some(0));
        assert_eq!(lan_rank(ip("172.20.0.3")), Some(1));
        assert_eq!(lan_rank(ip("10.8.0.2")), Some(2));
        assert_eq!(lan_rank(ip("172.32.0.1")), None);
        assert_eq!(lan_rank(ip("100.100.1.1")), None);
        assert_eq!(lan_rank(ip("8.8.8.8")), None);
        assert_eq!(lan_rank(ip("169.254.1.1")), None);
    }
}
