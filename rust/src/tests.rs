//! Проверки на двух-трёх узлах в одном процессе (без ретранслятора,
//! по прямым адресам).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::node::{Config, Node, Relays, close_code};
use crate::pairing::PairMode;
use crate::proto::{ALPN, Msg, read_msg, write_msg};
use crate::util::random_id;

async fn make_node(name: &str) -> (Arc<Node>, PathBuf) {
    let base = std::env::temp_dir().join(format!("osh-test-{name}-{}", random_id()));
    let cfg = Config {
        data_dir: base.join("data"),
        download_dir: base.join("dl"),
        device_name: name.into(),
        platform: "linux".into(),
        relays: Relays::Disabled,
        mdns: false,
    };
    let node = Node::start(cfg).await.expect("start");
    for _ in 0..100 {
        if node.ep.addr().ip_addrs().next().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    (node, base)
}

async fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("не дождались: {what}");
}

fn events(node: &Node, into: &mut Vec<Value>) {
    if let Some(list) = node.events.poll()["events"].as_array() {
        into.extend(list.iter().cloned());
    }
}

fn transfer(node: &Node, id: &str) -> Value {
    node.snapshot()["transfers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == id)
        .cloned()
        .unwrap_or(Value::Null)
}

fn peer_ids(node: &Node) -> Vec<(String, String, bool)> {
    node.snapshot()["peers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["id"].as_str().unwrap().to_string(),
                p["kind"].as_str().unwrap().to_string(),
                p["online"].as_bool().unwrap(),
            )
        })
        .collect()
}

/// Постоянное сопряжение: B сканирует код A, A подтверждает.
async fn pair(a: &Arc<Node>, b: &Arc<Node>) {
    let inv = a.create_invite(PairMode::Pair);
    let info = b.use_invite(inv["uri"].as_str().unwrap()).unwrap();
    wait_for("запрос на сопряжение", 10, || {
        !a.snapshot()["requests"].as_array().unwrap().is_empty()
    })
    .await;
    let snap = a.snapshot();
    let req = &snap["requests"][0];
    assert_eq!(req["kind"], "pair");
    assert_eq!(req["code"], info["code"], "проверочные коды на двух экранах совпадают");
    assert_eq!(req["name"], b.name());
    a.respond_pair(req["id"].as_str().unwrap(), true).unwrap();
    let mut ev = Vec::new();
    wait_for("итог сопряжения", 10, || {
        events(b, &mut ev);
        ev.iter().any(|e| e["type"] == "join_result")
    })
    .await;
    let res = ev.iter().find(|e| e["type"] == "join_result").unwrap();
    assert_eq!(res["ok"], true, "{res}");
    let (ida, idb) = (a.id().to_string(), b.id().to_string());
    wait_for("оба на связи", 10, || {
        peer_ids(a).iter().any(|p| p.0 == idb && p.1 == "mine" && p.2)
            && peer_ids(b).iter().any(|p| p.0 == ida && p.1 == "mine" && p.2)
    })
    .await;
}

fn write_tree(root: &Path) {
    std::fs::create_dir_all(root.join("Фото/2024")).unwrap();
    std::fs::write(root.join("Фото/a.jpg"), vec![7u8; 300_000]).unwrap();
    std::fs::write(root.join("Фото/2024/b.jpg"), b"bbb").unwrap();
    std::fs::write(root.join("Фото/пусто.txt"), b"").unwrap();
    std::fs::write(root.join("doc.txt"), b"hello").unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pair_send_text_and_forget() {
    let (a, base_a) = make_node("A").await;
    let (b, base_b) = make_node("B").await;
    pair(&a, &b).await;

    // Файлы и папка от B к A: принимаются без вопросов (свои устройства).
    let src = base_b.join("src");
    write_tree(&src);
    let tid = b.send_files(a.id(), vec![src.join("Фото"), src.join("doc.txt")]).unwrap();
    wait_for("передача файлов", 20, || {
        let t = transfer(&b, &tid);
        assert_ne!(t["status"], "failed", "{t}");
        t["status"] == "done"
    })
    .await;
    let dl = base_a.join("dl");
    wait_for("файлы на месте", 10, || dl.join("doc.txt").exists()).await;
    assert_eq!(std::fs::read(dl.join("Фото/a.jpg")).unwrap(), vec![7u8; 300_000]);
    assert_eq!(std::fs::read(dl.join("Фото/2024/b.jpg")).unwrap(), b"bbb");
    assert_eq!(std::fs::read(dl.join("Фото/пусто.txt")).unwrap(), b"");
    assert_eq!(std::fs::read(dl.join("doc.txt")).unwrap(), b"hello");
    let m_src = std::fs::metadata(src.join("doc.txt")).unwrap().modified().unwrap();
    let m_dst = std::fs::metadata(dl.join("doc.txt")).unwrap().modified().unwrap();
    let diff = m_src.duration_since(m_dst).or_else(|e| Ok::<_, ()>(e.duration())).unwrap();
    assert!(diff < Duration::from_secs(2), "время изменения сохраняется");
    // Временная папка убрана.
    let stage = dl.join(crate::transfer::STAGE_DIR);
    assert!(
        std::fs::read_dir(&stage)
            .map(|d| d.filter_map(|e| e.ok()).all(|e| !e.path().is_dir()))
            .unwrap_or(true)
    );

    // Повторная отправка той же папки: новое имя, старое не затирается.
    let tid2 = b.send_files(a.id(), vec![src.join("Фото")]).unwrap();
    wait_for("вторая передача", 20, || transfer(&b, &tid2)["status"] == "done").await;
    wait_for("Фото (1)", 10, || dl.join("Фото (1)/a.jpg").exists()).await;

    // Текст от A к B.
    let text = "Привет 👋\nhttps://ogneliss.ru";
    let t3 = a.send_text(b.id(), text.into()).unwrap();
    wait_for("текст", 10, || transfer(&a, &t3)["status"] == "done").await;
    let got = b.snapshot()["transfers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["kind"] == "text" && t["dir"] == "in")
        .cloned()
        .unwrap();
    assert_eq!(got["text"], text);

    // «Забыть устройство» на A — B узнаёт и тоже убирает A.
    a.remove_peer(&b.id()).unwrap();
    let mut ev = Vec::new();
    // Событие приходит чуть позже, чем A исчезает из списка, — ждём обоих.
    wait_for("B забыл A", 10, || {
        events(&b, &mut ev);
        peer_ids(&b).is_empty() && ev.iter().any(|e| e["type"] == "peer_removed")
    })
    .await;
    // И больше не может прислать файл.
    assert!(b.send_files(a.id(), vec![src.join("doc.txt")]).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn strangers_are_rejected() {
    let (a, _) = make_node("A").await;
    let (c, _) = make_node("C").await;

    // Без показанного QR-кода посторонний отклоняется сразу.
    let attempt = c.ep.connect(a.ep.addr(), ALPN).await;
    if let Ok(conn) = attempt {
        let res = async {
            let (mut s, mut r) = conn.open_bi().await?;
            write_msg(&mut s, &Msg::Text { id: "x".into(), text: "спам".into() }).await?;
            s.finish()?;
            read_msg(&mut r).await
        }
        .await;
        assert!(res.is_err(), "посторонний не должен получить ответ");
        assert_eq!(close_code(&conn), Some(403));
    }
    assert!(a.snapshot()["transfers"].as_array().unwrap().is_empty());

    // Пока QR-код показан, посторонний с неверным секретом получает отказ.
    let inv = a.create_invite(PairMode::Pair);
    let mut link = crate::pairing::PairLink::parse(inv["uri"].as_str().unwrap()).unwrap();
    link.token = [0u8; 16];
    c.use_invite(&link.to_uri()).unwrap();
    let mut ev = Vec::new();
    wait_for("отказ", 10, || {
        events(&c, &mut ev);
        ev.iter().any(|e| e["type"] == "join_result")
    })
    .await;
    let res = ev.iter().find(|e| e["type"] == "join_result").unwrap();
    assert_eq!(res["ok"], false);
    assert_eq!(res["error"], "expired", "{res}");
    assert!(a.snapshot()["requests"].as_array().unwrap().is_empty());
    assert!(peer_ids(&a).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn once_session_asks_before_accepting() {
    let (a, base_a) = make_node("A").await;
    let (b, base_b) = make_node("B").await;

    let inv = a.create_invite(PairMode::Once);
    b.use_invite(inv["uri"].as_str().unwrap()).unwrap();
    let mut ev = Vec::new();
    wait_for("разовый сеанс", 10, || {
        events(&b, &mut ev);
        ev.iter().any(|e| e["type"] == "join_result" && e["ok"] == true)
    })
    .await;
    assert_eq!(peer_ids(&a)[0].1, "session");
    assert_eq!(peer_ids(&b)[0].1, "session");
    assert_eq!(a.snapshot()["invite"]["joined"]["state"], "done");
    // Код одноразовый: второй раз им не воспользоваться.
    let (c, _) = make_node("C").await;
    c.use_invite(inv["uri"].as_str().unwrap()).unwrap();
    let mut ev_c = Vec::new();
    wait_for("второй вход отклонён", 10, || {
        events(&c, &mut ev_c);
        ev_c.iter().any(|e| e["type"] == "join_result")
    })
    .await;
    assert_eq!(ev_c.iter().find(|e| e["type"] == "join_result").unwrap()["ok"], false);

    let src = base_b.join("src");
    write_tree(&src);

    // Получатель отказывается.
    let t1 = b.send_files(a.id(), vec![src.join("doc.txt")]).unwrap();
    wait_for("вопрос «Принять?»", 10, || {
        a.snapshot()["requests"].as_array().unwrap().iter().any(|r| r["kind"] == "offer")
    })
    .await;
    let snap = a.snapshot();
    let req = snap["requests"].as_array().unwrap().iter().find(|r| r["kind"] == "offer").unwrap();
    assert_eq!(req["names"][0], "doc.txt");
    a.respond_offer(req["transfer"].as_str().unwrap(), false).unwrap();
    wait_for("отказ получателя", 10, || transfer(&b, &t1)["status"] == "declined").await;
    assert!(!base_a.join("dl/doc.txt").exists());

    // Получатель соглашается.
    let t2 = b.send_files(a.id(), vec![src.join("doc.txt")]).unwrap();
    wait_for("вопрос «Принять?» (2)", 10, || {
        a.snapshot()["requests"].as_array().unwrap().iter().any(|r| r["kind"] == "offer")
    })
    .await;
    let snap = a.snapshot();
    let req = snap["requests"].as_array().unwrap().iter().find(|r| r["kind"] == "offer").unwrap();
    a.respond_offer(req["transfer"].as_str().unwrap(), true).unwrap();
    wait_for("приём", 10, || transfer(&b, &t2)["status"] == "done").await;
    wait_for("файл", 10, || base_a.join("dl/doc.txt").exists()).await;

    // Завершить обмен — сеанс пропадает у обоих.
    b.remove_peer(&a.id()).unwrap();
    wait_for("сеанс закрыт", 10, || peer_ids(&a).is_empty()).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_and_verify() {
    let (a, base_a) = make_node("A").await;
    let (b, base_b) = make_node("B").await;
    pair(&a, &b).await;

    let src = base_b.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let body: Vec<u8> = (0..3_000_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    let file = src.join("video.bin");
    std::fs::write(&file, &body).unwrap();
    let metas: Vec<_> =
        crate::transfer::collect(std::slice::from_ref(&file)).unwrap().into_iter().map(|s| s.meta).collect();
    let stage = crate::transfer::stage_dir(&a.stage_root(), &b.id(), &metas);

    // 1) Недокачанный кусок испорчен — проверка хеша это ловит.
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join("0.part"), vec![0u8; 1_000_000]).unwrap();
    let t1 = b.send_files(a.id(), vec![file.clone()]).unwrap();
    wait_for("передача с порченым началом", 20, || {
        transfer(&b, &t1)["status"] == "failed"
    })
    .await;
    let t = transfer(&b, &t1);
    assert_eq!(t["error"], "verify", "{t}");
    assert_eq!(t["failed_files"], 1);
    assert!(!base_a.join("dl/video.bin").exists());

    // 2) Правильное начало — докачивается только остаток.
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join("0.part"), &body[..1_000_000]).unwrap();
    b.retry_transfer(&t1).unwrap();
    wait_for("докачка", 20, || transfer(&b, &t1)["status"] == "done").await;
    wait_for("файл", 10, || base_a.join("dl/video.bin").exists()).await;
    assert_eq!(std::fs::read(base_a.join("dl/video.bin")).unwrap(), body);
    assert!(!stage.exists());
}

/// В ссылке для сообщений нет IP-адресов — устройство находится по записи
/// в DNS. В тестах DNS нет, поэтому добавляем прямые адреса пригласившего.
fn reachable(a: &Node, url: &str) -> String {
    let mut link = crate::pairing::PairLink::parse(url).unwrap();
    link.addrs = a.ep.addr().ip_addrs().copied().collect();
    link.to_web_uri()
}

async fn join_result(node: &Node, what: &str) -> Value {
    let mut ev = Vec::new();
    wait_for(what, 15, || {
        events(node, &mut ev);
        ev.iter().any(|e| e["type"] == "join_result")
    })
    .await;
    ev.into_iter().find(|e| e["type"] == "join_result").unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn message_link_pairs_after_confirmation() {
    let (a, _) = make_node("A").await;
    let (b, _) = make_node("B").await;
    let (c, _) = make_node("C").await;

    // Ссылка, которую перехватил чужой: A отказывает — ссылка сгорает.
    let l1 = a.create_link(PairMode::Pair, vec![], false).unwrap();
    let url1 = l1["url"].as_str().unwrap().to_string();
    assert!(url1.starts_with(crate::pairing::WEB_PREFIX), "{url1}");
    assert!(!url1.contains("&a="), "в ссылке для сообщений нет IP-адресов: {url1}");
    c.use_invite(&reachable(&a, &url1)).unwrap();
    wait_for("запрос от C", 10, || !a.snapshot()["requests"].as_array().unwrap().is_empty()).await;
    let snap = a.snapshot();
    assert_eq!(snap["requests"][0]["via_link"], true);
    a.respond_pair(snap["requests"][0]["id"].as_str().unwrap(), false).unwrap();
    let r = join_result(&c, "отказ C").await;
    assert_eq!(r["ok"], false);
    assert_eq!(r["error"], "declined", "{r}");
    wait_for("ссылка сгорела", 10, || a.snapshot()["links"].as_array().unwrap().is_empty()).await;

    // Настоящий адресат вставляет сообщение целиком; A сверяет код и подтверждает.
    let l2 = a.create_link(PairMode::Pair, vec![], false).unwrap();
    let url2 = l2["url"].as_str().unwrap();
    let info = b.use_invite(&format!("Подключайся ко мне: {} — до завтра.", reachable(&a, url2))).unwrap();
    assert_eq!(info["remote"], true);
    wait_for("запрос от B", 10, || !a.snapshot()["requests"].as_array().unwrap().is_empty()).await;
    let snap = a.snapshot();
    let req = &snap["requests"][0];
    assert_eq!(req["code"], info["code"], "коды на двух экранах совпадают");
    a.respond_pair(req["id"].as_str().unwrap(), true).unwrap();
    let r = join_result(&b, "итог B").await;
    assert_eq!(r["ok"], true, "{r}");
    let idb = b.id().to_string();
    wait_for("связаны", 10, || peer_ids(&a).iter().any(|p| p.0 == idb && p.1 == "mine")).await;
    assert!(a.snapshot()["links"].as_array().unwrap().is_empty(), "ссылка одноразовая");

    // Повторно по израсходованной ссылке не войти.
    let (d, _) = make_node("D").await;
    d.use_invite(&reachable(&a, url2)).unwrap();
    assert_eq!(join_result(&d, "вход D").await["ok"], false);

    // Своё имя для связанного устройства — только у A.
    a.rename_peer(&b.id(), "Телефон мамы").unwrap();
    let p = a.snapshot()["peers"][0].clone();
    assert_eq!(p["name"], "Телефон мамы");
    assert_eq!(p["own_name"], b.name());
    assert_eq!(p["renamed"], true);
    a.rename_peer(&b.id(), "  ").unwrap();
    let p = a.snapshot()["peers"][0].clone();
    assert_eq!(p["name"], b.name());
    assert_eq!(p["renamed"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn files_by_link() {
    let (a, base_a) = make_node("A").await;
    let (b, base_b) = make_node("B").await;
    let (c, base_c) = make_node("C").await;

    let src = base_a.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("отчёт.pdf"), vec![5u8; 200_000]).unwrap();
    // Как на телефоне: выбранный файл — временная копия, её забирает ссылка.
    let link = a.create_link(PairMode::Files, vec![src.join("отчёт.pdf")], true).unwrap();
    assert!(!src.join("отчёт.pdf").exists(), "копия перенесена в папку ссылки");
    assert_eq!(link["count"], 1);
    assert_eq!(link["names"][0], "отчёт.pdf");
    let url = link["url"].as_str().unwrap().to_string();
    let owned = base_a.join("data/links").join(link["id"].as_str().unwrap());
    assert!(owned.exists());

    // Ссылкой могут воспользоваться несколько получателей; каждый соглашается на приём.
    for (node, base) in [(&b, &base_b), (&c, &base_c)] {
        node.use_invite(&reachable(&a, &url)).unwrap();
        assert_eq!(join_result(node, "вход по ссылке").await["ok"], true);
        wait_for("вопрос «Принять?»", 15, || {
            node.snapshot()["requests"].as_array().unwrap().iter().any(|r| r["kind"] == "offer")
        })
        .await;
        let snap = node.snapshot();
        let req = snap["requests"].as_array().unwrap().iter().find(|r| r["kind"] == "offer").unwrap();
        assert_eq!(req["names"][0], "отчёт.pdf");
        node.respond_offer(req["transfer"].as_str().unwrap(), true).unwrap();
        wait_for("файл получен", 15, || base.join("dl/отчёт.pdf").exists()).await;
        assert_eq!(std::fs::read(base.join("dl/отчёт.pdf")).unwrap(), vec![5u8; 200_000]);
    }
    assert_eq!(a.snapshot()["links"][0]["uses"], 2);

    // Отозванная ссылка больше не работает, копии файлов удалены.
    a.revoke_link(link["id"].as_str().unwrap()).unwrap();
    assert!(a.snapshot()["links"].as_array().unwrap().is_empty());
    wait_for("копии удалены", 10, || !owned.exists()).await;
    let (d, _) = make_node("D").await;
    d.use_invite(&reachable(&a, &url)).unwrap();
    assert_eq!(join_result(&d, "вход по отозванной ссылке").await["ok"], false);
}

#[test]
fn ffi_commands_work() {
    use std::ffi::{CStr, CString};
    let call = |req: serde_json::Value| -> Value {
        let c = CString::new(req.to_string()).unwrap();
        // SAFETY: строка с нулём на конце; результат освобождаем ниже.
        let out = unsafe { crate::osh_call(c.as_ptr()) };
        let s = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { crate::osh_free(out) };
        serde_json::from_str(&s).unwrap()
    };
    let base = std::env::temp_dir().join(format!("osh-ffi-{}", random_id()));
    assert_eq!(call(serde_json::json!({"cmd": "snapshot"}))["error"], "not_started");
    assert_eq!(call(serde_json::json!({"cmd": "nope"}))["ok"], false);
    let init = call(serde_json::json!({
        "cmd": "init",
        "data_dir": base.join("data").to_string_lossy(),
        "download_dir": base.join("dl").to_string_lossy(),
        "name": "Тестовый ноутбук",
        "platform": "linux",
    }));
    assert_eq!(init["ok"], true, "{init}");
    let again = call(serde_json::json!({
        "cmd": "init",
        "data_dir": base.join("data").to_string_lossy(),
        "download_dir": base.join("dl").to_string_lossy(),
    }));
    assert_eq!(again["result"]["resumed"], true);
    let snap = call(serde_json::json!({"cmd": "snapshot"}));
    assert_eq!(snap["result"]["me"]["name"], "Тестовый ноутбук");
    let inv = call(serde_json::json!({"cmd": "create_invite", "mode": "once"}));
    assert!(inv["result"]["uri"].as_str().unwrap().starts_with("ogshare://pair?"));
    assert_eq!(call(serde_json::json!({"cmd": "use_invite", "uri": "http://x"}))["error"], "bad_link");
    let own = call(serde_json::json!({"cmd": "use_invite", "uri": inv["result"]["uri"]}));
    assert_eq!(own["error"], "own_link");
    call(serde_json::json!({"cmd": "set_name", "name": "  Новое имя  "}));
    let snap = call(serde_json::json!({"cmd": "snapshot"}));
    assert_eq!(snap["result"]["me"]["name"], "Новое имя");
    let poll = crate::osh_poll();
    let p: Value = serde_json::from_str(&unsafe { CStr::from_ptr(poll) }.to_string_lossy()).unwrap();
    unsafe { crate::osh_free(poll) };
    assert!(p["v"].as_u64().unwrap() > 0);
    assert_eq!(call(serde_json::json!({"cmd": "shutdown"}))["ok"], true);
    let _ = std::fs::remove_dir_all(&base);
}

/// Скорость передачи между двумя узлами на одной машине:
/// `cargo test --release -- --ignored throughput --nocapture`
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn throughput() {
    let (a, base_a) = make_node("A").await;
    let (b, base_b) = make_node("B").await;
    pair(&a, &b).await;
    let src = base_b.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let size = 400 * 1024 * 1024;
    let chunk: Vec<u8> = (0..1024 * 1024u32).map(|i| (i.wrapping_mul(2654435761) >> 11) as u8).collect();
    {
        use std::io::Write;
        let mut f = std::fs::File::create(src.join("big.bin")).unwrap();
        for _ in 0..(size / chunk.len()) {
            f.write_all(&chunk).unwrap();
        }
    }
    let started = std::time::Instant::now();
    let t = b.send_files(a.id(), vec![src.join("big.bin")]).unwrap();
    wait_for("передача", 300, || transfer(&b, &t)["status"] == "done").await;
    let secs = started.elapsed().as_secs_f64();
    println!("{} МБ за {:.1} с — {:.0} МБ/с", size / 1024 / 1024, secs, size as f64 / 1024.0 / 1024.0 / secs);
    assert_eq!(std::fs::metadata(base_a.join("dl/big.bin")).unwrap().len(), size as u64);
    let _ = std::fs::remove_dir_all(&base_a);
    let _ = std::fs::remove_dir_all(&base_b);
}
