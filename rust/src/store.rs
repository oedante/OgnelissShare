//! Хранение на устройстве: ключ устройства и список связанных устройств.
//!
//! - `identity.key` — 32 байта секретного ключа Ed25519. Создаётся при первом
//!   запуске и никуда не передаётся; открытый ключ из него — адрес устройства.
//! - `state.json` — имя устройства, «Мои устройства» и «Чужие устройства»,
//!   действующие ссылки-приглашения (с их секретами, поэтому доступ — только
//!   владельцу).
//!   Записывается атомарно (временный файл и переименование), чтобы сбой не
//!   испортил список.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use iroh::{EndpointAddr, EndpointId, SecretKey};
use serde::{Deserialize, Serialize};

const KEY_FILE: &str = "identity.key";
const STATE_FILE: &str = "state.json";

/// Насколько доверяем связанному устройству — решает каждая сторона сама.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Trust {
    /// «Мои устройства»: файлы принимаются без вопросов.
    #[default]
    Mine,
    /// «Чужие устройства»: связь постоянная, но каждую передачу файлов
    /// получатель подтверждает.
    Other,
}

impl Trust {
    pub fn as_str(self) -> &'static str {
        match self {
            Trust::Mine => "mine",
            Trust::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "mine" => Some(Trust::Mine),
            "other" => Some(Trust::Other),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PeerRecord {
    pub id: EndpointId,
    pub name: String,
    pub platform: String,
    /// Когда связали (секунды Unix).
    pub added: u64,
    /// Когда последний раз были на связи.
    #[serde(default)]
    pub last_seen: u64,
    /// Последний известный адрес: ретранслятор и прямые адреса.
    #[serde(default)]
    pub addr: Option<EndpointAddr>,
    /// Имя, которое дал этому устройству владелец (видно только здесь);
    /// пустое — имя, которое сообщает само устройство.
    #[serde(default)]
    pub alias: String,
    /// Группа: мои или чужие устройства. В прежних версиях все связанные
    /// устройства были «моими» — так и читаются.
    #[serde(default)]
    pub trust: Trust,
}

impl PeerRecord {
    /// Имя для показа: своё, если задано, иначе то, что сообщило устройство.
    pub fn display_name(&self) -> &str {
        if self.alias.is_empty() { &self.name } else { &self.alias }
    }
}

/// Ссылка-приглашение для сообщений (почта, мессенджер): живёт сутки и
/// переживает перезапуск приложения.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LinkInvite {
    pub id: String,
    /// pair — «Моё устройство», other — «Чужое устройство», once — разовый
    /// обмен, files — файлы по ссылке.
    pub mode: String,
    /// Секрет ссылки (base32).
    pub token: String,
    pub created: u64,
    /// Секунды Unix.
    pub expires: u64,
    /// Файлы для «files».
    #[serde(default)]
    pub files: Vec<PathBuf>,
    /// Папка, куда перенесены копии файлов (телефоны), — удаляется вместе со ссылкой.
    #[serde(default)]
    pub owned_dir: Option<PathBuf>,
    /// Сколько раз по ссылке получили файлы.
    #[serde(default)]
    pub uses: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Saved {
    #[serde(default)]
    pub version: u32,
    /// Имя, которое выбрал пользователь; пустое — имя системы.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub peers: Vec<PeerRecord>,
    /// Папка для полученных файлов, если пользователь выбрал свою.
    #[serde(default)]
    pub download_dir: String,
    /// Действующие ссылки-приглашения.
    #[serde(default)]
    pub invites: Vec<LinkInvite>,
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn open(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        Ok(Store { dir: dir.to_path_buf() })
    }

    /// Ключ устройства: читаем или создаём. Повреждённый файл не перезаписываем
    /// молча — откладываем в сторону, чтобы его можно было восстановить.
    pub fn load_or_create_key(&self) -> Result<SecretKey> {
        let path = self.dir.join(KEY_FILE);
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice()) {
                return Ok(SecretKey::from_bytes(&arr));
            }
            let aside = self.dir.join(format!("{KEY_FILE}.broken-{}", crate::util::now_unix()));
            let _ = fs::rename(&path, aside);
        }
        let key = SecretKey::generate();
        write_atomic(&path, &key.to_bytes(), true)?;
        Ok(key)
    }

    pub fn load_state(&self) -> Saved {
        let path = self.dir.join(STATE_FILE);
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Saved>(&bytes) {
                Ok(mut s) => {
                    // Повторы одного ключа (на всякий случай) схлопываем.
                    let mut seen = std::collections::HashSet::new();
                    s.peers.retain(|p| seen.insert(p.id));
                    s
                }
                Err(_) => {
                    let aside = self.dir.join(format!("{STATE_FILE}.broken-{}", crate::util::now_unix()));
                    let _ = fs::rename(&path, aside);
                    Saved::default()
                }
            },
            Err(_) => Saved::default(),
        }
    }

    pub fn save_state(&self, saved: &Saved) -> Result<()> {
        let mut s = saved.clone();
        s.version = 1;
        let body = serde_json::to_vec_pretty(&s)?;
        // В файле секреты ссылок — права только у владельца.
        write_atomic(&self.dir.join(STATE_FILE), &body, true)
    }
}

fn write_atomic(path: &Path, body: &[u8], secret: bool) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if secret {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = secret;
        let mut f = opts.open(&tmp).with_context(|| format!("open {}", tmp.display()))?;
        f.write_all(body)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_and_state_persist() {
        let dir = std::env::temp_dir().join(format!("osh-store-{}", crate::util::random_id()));
        let store = Store::open(&dir).unwrap();
        let k1 = store.load_or_create_key().unwrap();
        let k2 = store.load_or_create_key().unwrap();
        assert_eq!(k1.public(), k2.public());

        let peer = SecretKey::generate().public();
        let saved = Saved {
            version: 0,
            name: "Мой ноутбук".into(),
            download_dir: String::new(),
            peers: vec![PeerRecord {
                id: peer,
                name: "Телефон".into(),
                platform: "android".into(),
                added: 1,
                last_seen: 2,
                addr: Some(EndpointAddr::new(peer)),
                alias: "Телефон мамы".into(),
                trust: Trust::Other,
            }],
            invites: vec![LinkInvite {
                id: "a1".into(),
                mode: "files".into(),
                token: "secret".into(),
                created: 1,
                expires: 2,
                files: vec![PathBuf::from("/tmp/a.txt")],
                owned_dir: None,
                uses: 0,
            }],
        };
        store.save_state(&saved).unwrap();
        let back = store.load_state();
        assert_eq!(back.name, "Мой ноутбук");
        assert_eq!(back.peers.len(), 1);
        assert_eq!(back.peers[0].id, peer);
        assert_eq!(back.peers[0].display_name(), "Телефон мамы");
        assert_eq!(back.peers[0].trust, Trust::Other);
        assert_eq!(back.invites.len(), 1);
        assert_eq!(back.invites[0].files, vec![PathBuf::from("/tmp/a.txt")]);
        // Старый файл без новых полей читается; прежние устройства — «мои».
        let old = format!(
            r#"{{"name":"x","peers":[{{"id":"{peer}","name":"n","platform":"ios","added":1}}]}}"#
        );
        fs::write(dir.join(STATE_FILE), old).unwrap();
        let back = store.load_state();
        assert!(back.invites.is_empty());
        assert_eq!(back.peers[0].trust, Trust::Mine);

        fs::write(dir.join(STATE_FILE), b"{oops").unwrap();
        assert!(store.load_state().peers.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
