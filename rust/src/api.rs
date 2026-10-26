//! Команды интерфейса: JSON на входе, JSON на выходе.
//!
//! Вызовы синхронные и быстрые: всё долгое (соединения, передачи) уходит
//! в фоновые задачи, а итог приходит событиями через `osh_poll`.
//!
//! Ответ: `{"ok": true, "result": …}` или `{"ok": false, "error": "<код>",
//! "detail": "<подробности для журнала>"}`.

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use iroh::EndpointId;
use serde_json::{Value, json};
use tokio::runtime::Runtime;

use crate::node::{Config, Node, Relays, VERSION, error_code};
use crate::pairing::PairMode;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static NODE: Mutex<Option<Arc<Node>>> = Mutex::new(None);

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(3)
            .thread_name("ogneliss-share")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

fn node() -> Result<Arc<Node>> {
    NODE.lock().ok().and_then(|n| n.clone()).ok_or_else(|| anyhow!("not_started"))
}

pub fn call(request: &str) -> String {
    let res =
        serde_json::from_str::<Value>(request).map_err(|_| anyhow!("bad_request")).and_then(|v| dispatch(&v));
    match res {
        Ok(result) => json!({ "ok": true, "result": result }),
        Err(e) => json!({ "ok": false, "error": error_code(&e), "detail": format!("{e:#}") }),
    }
    .to_string()
}

pub fn poll() -> String {
    match node() {
        Ok(n) => n.events.poll().to_string(),
        Err(_) => json!({ "v": 0, "events": [] }).to_string(),
    }
}

fn str_arg<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key).and_then(|x| x.as_str()).ok_or_else(|| anyhow!("bad_request"))
}

fn peer_arg(v: &Value) -> Result<EndpointId> {
    EndpointId::from_str(str_arg(v, "peer")?).map_err(|_| anyhow!("not_found"))
}

fn paths_arg(v: &Value) -> Vec<PathBuf> {
    v.get("paths")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(PathBuf::from).collect())
        .unwrap_or_default()
}

fn ok() -> Result<Value> {
    Ok(Value::Null)
}

fn dispatch(v: &Value) -> Result<Value> {
    let cmd = str_arg(v, "cmd")?;
    match cmd {
        "init" => return init(v),
        "version" => return Ok(json!(VERSION)),
        _ => {}
    }
    let node = node()?;
    // Команды запускают фоновые задачи — им нужен контекст среды выполнения.
    let _rt = runtime().enter();
    match cmd {
        "snapshot" => Ok(node.snapshot()),
        "set_name" => {
            node.set_name(str_arg(v, "name")?);
            ok()
        }
        "set_active" => {
            node.set_active(v.get("active").and_then(Value::as_bool).unwrap_or(true));
            ok()
        }
        "refresh" => {
            node.refresh();
            ok()
        }
        "set_download_dir" => {
            let path = str_arg(v, "path")?.trim();
            node.set_download_dir(if path.is_empty() { None } else { Some(PathBuf::from(path)) });
            ok()
        }
        "create_invite" => {
            let mode = match PairMode::parse(str_arg(v, "mode")?) {
                Some(m @ (PairMode::Pair | PairMode::Once)) => m,
                _ => bail!("bad_request"),
            };
            Ok(node.create_invite(mode))
        }
        "create_link" => {
            let mode = PairMode::parse(str_arg(v, "mode")?).ok_or_else(|| anyhow!("bad_request"))?;
            let take = v.get("take_files").and_then(Value::as_bool).unwrap_or(false);
            node.create_link(mode, paths_arg(v), take)
        }
        "revoke_link" => {
            node.revoke_link(str_arg(v, "id")?)?;
            ok()
        }
        "rename_peer" => {
            node.rename_peer(&peer_arg(v)?, str_arg(v, "name")?)?;
            ok()
        }
        "cancel_invite" => {
            node.cancel_invite();
            ok()
        }
        "use_invite" => node.use_invite(str_arg(v, "uri")?),
        "respond_pair" => {
            let accept = v.get("accept").and_then(Value::as_bool).unwrap_or(false);
            node.respond_pair(str_arg(v, "id")?, accept)?;
            ok()
        }
        "remove_peer" => {
            node.remove_peer(&peer_arg(v)?)?;
            ok()
        }
        "send_files" => Ok(json!(node.send_files(peer_arg(v)?, paths_arg(v))?)),
        "send_text" => Ok(json!(node.send_text(peer_arg(v)?, str_arg(v, "text")?.to_string())?)),
        "respond_offer" => {
            let accept = v.get("accept").and_then(Value::as_bool).unwrap_or(false);
            node.respond_offer(str_arg(v, "id")?, accept)?;
            ok()
        }
        "cancel_transfer" => {
            node.cancel_transfer(str_arg(v, "id")?)?;
            ok()
        }
        "retry_transfer" => {
            node.retry_transfer(str_arg(v, "id")?)?;
            ok()
        }
        "dismiss_transfer" => {
            node.dismiss_transfer(str_arg(v, "id")?);
            ok()
        }
        "clear_transfers" => {
            node.clear_transfers();
            ok()
        }
        "get_text" => Ok(json!(node.get_text(str_arg(v, "id")?))),
        "logs" => Ok(json!(node.events.log_lines())),
        "shutdown" => {
            let n = node.clone();
            drop(_rt);
            runtime().block_on(async move {
                let _ = tokio::time::timeout(Duration::from_secs(3), n.shutdown()).await;
            });
            if let Ok(mut slot) = NODE.lock() {
                *slot = None;
            }
            ok()
        }
        _ => bail!("bad_request"),
    }
}

fn init(v: &Value) -> Result<Value> {
    let data_dir = PathBuf::from(str_arg(v, "data_dir")?);
    let download_dir = PathBuf::from(str_arg(v, "download_dir")?);
    let name = v.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let platform = crate::util::clean_platform(v.get("platform").and_then(Value::as_str).unwrap_or(""));

    // Повторный вызов (перезапуск интерфейса без перезапуска процесса).
    if let Ok(n) = node() {
        return Ok(json!({ "id": n.id().to_string(), "resumed": true }));
    }
    let cfg = Config {
        data_dir,
        download_dir,
        device_name: name,
        platform,
        relays: Relays::from_build(),
        mdns: true,
    };
    let started =
        runtime().block_on(async { tokio::time::timeout(Duration::from_secs(20), Node::start(cfg)).await });
    let node = match started {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => return Err(e.context("start")),
        Err(_) => bail!("timeout"),
    };
    let id = node.id().to_string();
    if let Ok(mut slot) = NODE.lock() {
        *slot = Some(node);
    }
    Ok(json!({ "id": id, "resumed": false }))
}
