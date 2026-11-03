//! Проверки службы связи через `Router` напрямую (без сети).
//!
//! Тесты со временем идут на «замороженных» часах tokio (`start_paused`):
//! `sleep` и `advance` двигают время мгновенно, ждать по-настоящему не нужно.
//! Исключение — `empty_poll_returns_quickly`, он меряет реальное время.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use ogneliss_signal::{AppState, Config, MAX_BODY, router};
use serde_json::{Value, json};
use tokio::time::{Instant, advance, sleep};
use tower::ServiceExt;

const OWNER: &str = "owner-secret-0123456789";
const OTHER: &str = "other-secret-9876543210";
const ROOM: &str = "room-aaaaaaaa";

struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Resp {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("тело — JSON")
    }
}

fn app_with(cfg: Config) -> (Router, Arc<AppState>) {
    let state = AppState::new(cfg);
    (router(state.clone()), state)
}

fn app() -> (Router, Arc<AppState>) {
    app_with(Config::default())
}

fn request(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Request<Body> {
    let mut b = Request::builder().method(method).uri(format!("/share/api/v1{path}"));
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    b.body(Body::from(body.to_owned())).unwrap()
}

async fn run(app: &Router, req: Request<Body>) -> Resp {
    let resp = app.clone().oneshot(req).await.unwrap();
    let (parts, body) = resp.into_parts();
    let body = body.collect().await.unwrap().to_bytes().to_vec();
    Resp { status: parts.status, headers: parts.headers, body }
}

async fn send(app: &Router, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Resp {
    run(app, request(method, path, headers, body)).await
}

async fn create_room(app: &Router, room: &str, owner: &str) -> Resp {
    send(app, "PUT", &format!("/rooms/{room}"), &[("x-owner", owner)], r#"{"ttl":600}"#).await
}

async fn post_offer(app: &Router, room: &str, msg: &str) -> Resp {
    send(app, "POST", &format!("/rooms/{room}/offers"), &[], &json!({ "msg": msg }).to_string()).await
}

/// Создаёт предложение и возвращает его `sid`.
async fn offer_sid(app: &Router, room: &str, msg: &str) -> String {
    let r = post_offer(app, room, msg).await;
    assert_eq!(r.status, StatusCode::CREATED);
    r.json()["sid"].as_str().unwrap().to_owned()
}

async fn put_answer(app: &Router, room: &str, sid: &str, owner: &str, msg: &str) -> Resp {
    let path = format!("/rooms/{room}/answers/{sid}");
    send(app, "PUT", &path, &[("x-owner", owner)], &json!({ "msg": msg }).to_string()).await
}

async fn get_offers(app: &Router, room: &str, owner: &str, query: &str) -> Resp {
    send(app, "GET", &format!("/rooms/{room}/offers?{query}"), &[("x-owner", owner)], "").await
}

async fn get_answer(app: &Router, room: &str, sid: &str, query: &str) -> Resp {
    send(app, "GET", &format!("/rooms/{room}/answers/{sid}?{query}"), &[], "").await
}

fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

#[tokio::test]
async fn health_ok_and_no_store() {
    let (app, _) = app();
    let r = send(&app, "GET", "/health", &[], "").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body, b"ok");
    assert_eq!(r.headers["cache-control"], "no-store");
}

#[tokio::test]
async fn no_store_on_every_response() {
    let (app, _) = app();
    let cases = [
        send(&app, "GET", "/rooms/short", &[], "").await, // 405
        send(&app, "GET", &format!("/rooms/{ROOM}/offers"), &[], "").await, // 404
        send(&app, "PUT", "/rooms/bad!", &[], "{}").await, // 400
        run(&app, Request::get("/nope").body(Body::empty()).unwrap()).await, // 404 вне API
    ];
    for r in cases {
        assert_eq!(r.headers["cache-control"], "no-store", "статус {}", r.status);
    }
}

#[tokio::test]
async fn create_room_returns_expiry_and_renews() {
    let (app, state) = app();
    let r = create_room(&app, ROOM, OWNER).await;
    assert_eq!(r.status, StatusCode::OK);
    let expires = r.json()["expires"].as_u64().unwrap();
    assert!(expires.abs_diff(now_unix() + 600) <= 2);
    assert_eq!(state.rooms(), 1);

    // Продление тем же владельцем.
    let r = send(&app, "PUT", &format!("/rooms/{ROOM}"), &[("x-owner", OWNER)], r#"{"ttl":3600}"#).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.json()["expires"].as_u64().unwrap() > expires + 2000);
    assert_eq!(state.rooms(), 1);
}

#[tokio::test]
async fn room_taken_by_other_owner_is_409() {
    let (app, _) = app();
    create_room(&app, ROOM, OWNER).await;
    assert_eq!(create_room(&app, ROOM, OTHER).await.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn bad_ttl_and_body_and_owner_are_400() {
    let (app, _) = app();
    let path = format!("/rooms/{ROOM}");
    let h = [("x-owner", OWNER)];
    for body in [r#"{"ttl":59}"#, r#"{"ttl":3601}"#, r#"{"ttl":-1}"#, r#"{"ttl":"60"}"#, "{}", "", "не json"]
    {
        assert_eq!(send(&app, "PUT", &path, &h, body).await.status, StatusCode::BAD_REQUEST, "{body}");
    }
    // Границы допустимы.
    assert_eq!(send(&app, "PUT", &path, &h, r#"{"ttl":60}"#).await.status, StatusCode::OK);
    assert_eq!(send(&app, "PUT", &path, &h, r#"{"ttl":3600}"#).await.status, StatusCode::OK);
    // Нет секрета владельца или он слишком короткий.
    let ok = r#"{"ttl":600}"#;
    assert_eq!(send(&app, "PUT", "/rooms/room-bbbbbbbb", &[], ok).await.status, StatusCode::BAD_REQUEST);
    let short = [("x-owner", "short")];
    assert_eq!(send(&app, "PUT", "/rooms/room-bbbbbbbb", &short, ok).await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn delete_room_checks_owner() {
    let (app, state) = app();
    create_room(&app, ROOM, OWNER).await;
    let path = format!("/rooms/{ROOM}");
    assert_eq!(send(&app, "DELETE", &path, &[("x-owner", OTHER)], "").await.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&app, "DELETE", &path, &[], "").await.status, StatusCode::FORBIDDEN);
    assert_eq!(state.rooms(), 1);
    assert_eq!(send(&app, "DELETE", &path, &[("x-owner", OWNER)], "").await.status, StatusCode::NO_CONTENT);
    assert_eq!(state.rooms(), 0);
    assert_eq!(send(&app, "DELETE", &path, &[("x-owner", OWNER)], "").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_room_is_404() {
    let (app, _) = app();
    let sid = "sid-aaaaaaaa";
    assert_eq!(post_offer(&app, ROOM, "x").await.status, StatusCode::NOT_FOUND);
    assert_eq!(get_offers(&app, ROOM, OWNER, "wait=0").await.status, StatusCode::NOT_FOUND);
    assert_eq!(get_answer(&app, ROOM, sid, "wait=0").await.status, StatusCode::NOT_FOUND);
    assert_eq!(put_answer(&app, ROOM, sid, OWNER, "x").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_ids_are_400() {
    let (app, _) = app();
    let long = "a".repeat(65);
    for id in ["short", "1234567", "bad!bad!bad!", "пробел-пробел", long.as_str()] {
        let enc = id.replace('!', "%21");
        let h = [("x-owner", OWNER)];
        assert_eq!(
            send(&app, "PUT", &format!("/rooms/{enc}"), &h, r#"{"ttl":60}"#).await.status,
            StatusCode::BAD_REQUEST,
            "PUT {id}"
        );
        assert_eq!(
            send(&app, "DELETE", &format!("/rooms/{enc}"), &h, "").await.status,
            StatusCode::BAD_REQUEST,
            "DELETE {id}"
        );
        assert_eq!(post_offer(&app, &enc, "x").await.status, StatusCode::BAD_REQUEST, "POST {id}");
        assert_eq!(
            get_offers(&app, &enc, OWNER, "wait=0").await.status,
            StatusCode::BAD_REQUEST,
            "GET offers {id}"
        );
        assert_eq!(
            get_answer(&app, &enc, "sid-aaaaaaaa", "wait=0").await.status,
            StatusCode::BAD_REQUEST,
            "GET answer {id}"
        );
        assert_eq!(
            get_answer(&app, ROOM, &enc, "wait=0").await.status,
            StatusCode::BAD_REQUEST,
            "GET sid {id}"
        );
        assert_eq!(
            put_answer(&app, ROOM, &enc, OWNER, "x").await.status,
            StatusCode::BAD_REQUEST,
            "PUT sid {id}"
        );
    }
    // Границы длины и весь алфавит допустимы.
    for id in ["abcdefgh", &"A_-9".repeat(16)] {
        assert_eq!(create_room(&app, id, OWNER).await.status, StatusCode::OK, "{id}");
    }
}

#[tokio::test(start_paused = true)]
async fn offer_reaches_owner_long_poll_with_seq() {
    let (app, _) = app();
    create_room(&app, ROOM, OWNER).await;

    // Владелец уже ждёт, когда приходит предложение.
    let poll = tokio::spawn({
        let app = app.clone();
        async move { get_offers(&app, ROOM, OWNER, "after=0&wait=20").await }
    });
    sleep(Duration::from_millis(50)).await;
    assert!(!poll.is_finished());
    let sid = offer_sid(&app, ROOM, "зашифрованное-sdp-1").await;
    assert_eq!(sid.len(), 12);

    let r = poll.await.unwrap();
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json(), json!({"offers": [{"seq": 1, "sid": sid, "msg": "зашифрованное-sdp-1"}]}));

    // Второе предложение: `after` отсекает уже виденное, seq растёт.
    let sid2 = offer_sid(&app, ROOM, "sdp-2").await;
    let r = get_offers(&app, ROOM, OWNER, "after=1&wait=0").await;
    assert_eq!(r.json(), json!({"offers": [{"seq": 2, "sid": sid2, "msg": "sdp-2"}]}));
    let r = get_offers(&app, ROOM, OWNER, "wait=0").await;
    assert_eq!(r.json()["offers"].as_array().unwrap().len(), 2);
    let r = get_offers(&app, ROOM, OWNER, "after=2&wait=0").await;
    assert_eq!(r.json(), json!({"offers": []}));
}

#[tokio::test]
async fn offers_are_owner_only() {
    let (app, _) = app();
    create_room(&app, ROOM, OWNER).await;
    offer_sid(&app, ROOM, "sdp").await;
    assert_eq!(get_offers(&app, ROOM, OTHER, "wait=0").await.status, StatusCode::FORBIDDEN);
    let r = send(&app, "GET", &format!("/rooms/{ROOM}/offers?wait=0"), &[], "").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // Содержимое чужому не утекает.
    assert!(!String::from_utf8_lossy(&r.body).contains("sdp"));
}

#[tokio::test(start_paused = true)]
async fn answer_reaches_browser_long_poll() {
    let (app, _) = app();
    create_room(&app, ROOM, OWNER).await;
    let sid = offer_sid(&app, ROOM, "offer").await;

    let poll = tokio::spawn({
        let (app, sid) = (app.clone(), sid.clone());
        async move { get_answer(&app, ROOM, &sid, "wait=20").await }
    });
    sleep(Duration::from_millis(50)).await;
    assert!(!poll.is_finished());

    // Чужой ответить не может, незнакомый sid — 404.
    assert_eq!(put_answer(&app, ROOM, &sid, OTHER, "evil").await.status, StatusCode::FORBIDDEN);
    assert_eq!(put_answer(&app, ROOM, "sid-zzzzzzzz", OWNER, "x").await.status, StatusCode::NOT_FOUND);
    assert!(!poll.is_finished());

    assert_eq!(put_answer(&app, ROOM, &sid, OWNER, "answer").await.status, StatusCode::NO_CONTENT);
    let r = poll.await.unwrap();
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json(), json!({"msg": "answer"}));

    // Ответ можно забрать повторно, а повторный PUT его заменяет.
    assert_eq!(get_answer(&app, ROOM, &sid, "wait=0").await.json(), json!({"msg": "answer"}));
    put_answer(&app, ROOM, &sid, OWNER, "answer-2").await;
    assert_eq!(get_answer(&app, ROOM, &sid, "wait=0").await.json(), json!({"msg": "answer-2"}));
}

#[tokio::test]
async fn unknown_sid_is_404_and_pending_is_204() {
    let (app, _) = app();
    create_room(&app, ROOM, OWNER).await;
    let sid = offer_sid(&app, ROOM, "offer").await;
    assert_eq!(get_answer(&app, ROOM, "sid-zzzzzzzz", "wait=0").await.status, StatusCode::NOT_FOUND);
    assert_eq!(get_answer(&app, ROOM, &sid, "wait=0").await.status, StatusCode::NO_CONTENT);
}

/// Единственный тест на настоящих часах: пустой опрос с `wait=1` отвечает
/// примерно через секунду, а не висит.
#[tokio::test]
async fn empty_poll_returns_quickly() {
    let (app, _) = app();
    create_room(&app, ROOM, OWNER).await;
    let sid = offer_sid(&app, ROOM, "offer").await;

    let t = std::time::Instant::now();
    let r = get_offers(&app, ROOM, OWNER, "after=1&wait=1").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json(), json!({"offers": []}));
    assert!(t.elapsed() >= Duration::from_millis(900) && t.elapsed() < Duration::from_secs(3));

    let t = std::time::Instant::now();
    let r = get_answer(&app, ROOM, &sid, "wait=1").await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(r.body.is_empty());
    assert!(t.elapsed() >= Duration::from_millis(900) && t.elapsed() < Duration::from_secs(3));
}

#[tokio::test(start_paused = true)]
async fn wait_defaults_to_25_and_is_capped_at_30() {
    let (app, _) = app();
    create_room(&app, ROOM, OWNER).await;
    let sid = offer_sid(&app, ROOM, "offer").await;
    // Комната живёт 600 с, так что опрос не оборвётся истечением.
    for (query, secs) in [("", 25), ("wait=999", 30), ("wait=5", 5), ("wait=0", 0)] {
        let t = Instant::now();
        let r = get_answer(&app, ROOM, &sid, query).await;
        assert_eq!(r.status, StatusCode::NO_CONTENT);
        let d = t.elapsed();
        assert!(d >= Duration::from_secs(secs) && d < Duration::from_secs(secs + 1), "{query}: {d:?}");
    }
    assert_eq!(get_answer(&app, ROOM, &sid, "wait=abc").await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn body_over_16_kib_is_413() {
    let (app, _) = app();
    create_room(&app, ROOM, OWNER).await;
    let msg = |n: usize| format!(r#"{{"msg":"{}"}}"#, "a".repeat(n));

    // Ровно 16 КиБ проходит.
    let exact = msg(MAX_BODY - 10);
    assert_eq!(exact.len(), MAX_BODY);
    let path = format!("/rooms/{ROOM}/offers");
    assert_eq!(send(&app, "POST", &path, &[], &exact).await.status, StatusCode::CREATED);

    // На байт больше — 413, и без длины в заголовке (chunked), и с ней.
    let big = msg(MAX_BODY - 9);
    assert_eq!(big.len(), MAX_BODY + 1);
    assert_eq!(send(&app, "POST", &path, &[], &big).await.status, StatusCode::PAYLOAD_TOO_LARGE);
    let len = big.len().to_string();
    let r = send(&app, "POST", &path, &[("content-length", &len)], &big).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(r.headers["cache-control"], "no-store");
    // Предел касается и остальных запросов с телом.
    let h = [("x-owner", OWNER)];
    assert_eq!(
        send(&app, "PUT", &format!("/rooms/{ROOM}"), &h, &big).await.status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let sid = offer_sid(&app, ROOM, "x").await;
    assert_eq!(
        send(&app, "PUT", &format!("/rooms/{ROOM}/answers/{sid}"), &h, &big).await.status,
        StatusCode::PAYLOAD_TOO_LARGE
    );
}

#[tokio::test]
async fn bad_messages_are_400() {
    let (app, _) = app();
    create_room(&app, ROOM, OWNER).await;
    let path = format!("/rooms/{ROOM}/offers");
    for body in ["", "{}", r#"{"msg":""}"#, r#"{"msg":5}"#, r#"{"msg":null}"#, "[]"] {
        assert_eq!(send(&app, "POST", &path, &[], body).await.status, StatusCode::BAD_REQUEST, "{body}");
    }
    // Content-Type не нужен: браузер может прислать text/plain.
    let h = [("content-type", "text/plain")];
    assert_eq!(send(&app, "POST", &path, &h, r#"{"msg":"x"}"#).await.status, StatusCode::CREATED);
}

#[tokio::test]
async fn sixty_fifth_offer_is_429() {
    let (app, _) = app();
    create_room(&app, ROOM, OWNER).await;
    for i in 1..=64 {
        assert_eq!(post_offer(&app, ROOM, &format!("sdp-{i}")).await.status, StatusCode::CREATED, "{i}");
    }
    assert_eq!(post_offer(&app, ROOM, "sdp-65").await.status, StatusCode::TOO_MANY_REQUESTS);
    // В другую комнату по-прежнему можно.
    create_room(&app, "room-bbbbbbbb", OWNER).await;
    assert_eq!(post_offer(&app, "room-bbbbbbbb", "sdp").await.status, StatusCode::CREATED);
}

#[tokio::test(start_paused = true)]
async fn rate_limit_per_ip_is_shared_by_put_and_post() {
    let (app, _) = app_with(Config { rate_limit: 3, ..Config::default() });
    let ip = |v: &'static str| [("x-real-ip", v), ("x-owner", OWNER)];
    let put = |h: [(&'static str, &'static str); 2]| {
        let app = app.clone();
        async move { send(&app, "PUT", &format!("/rooms/{ROOM}"), &h, r#"{"ttl":600}"#).await }
    };
    let offer = |ip: &'static str| {
        let app = app.clone();
        async move {
            let path = format!("/rooms/{ROOM}/offers");
            send(&app, "POST", &path, &[("x-real-ip", ip)], r#"{"msg":"x"}"#).await
        }
    };

    assert_eq!(put(ip("203.0.113.1")).await.status, StatusCode::OK);
    assert_eq!(offer("203.0.113.1").await.status, StatusCode::CREATED);
    assert_eq!(put(ip("203.0.113.1")).await.status, StatusCode::OK);
    let r = offer("203.0.113.1").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.headers["cache-control"], "no-store");
    assert_eq!(put(ip("203.0.113.1")).await.status, StatusCode::TOO_MANY_REQUESTS);

    // Другой IP не затронут; чтение (GET) не считается.
    assert_eq!(offer("203.0.113.2").await.status, StatusCode::CREATED);
    for _ in 0..5 {
        assert_eq!(get_offers(&app, ROOM, OWNER, "wait=0").await.status, StatusCode::OK);
    }

    // Окно прошло — снова можно.
    advance(Duration::from_secs(61)).await;
    assert_eq!(offer("203.0.113.1").await.status, StatusCode::CREATED);
}

#[tokio::test]
async fn client_ip_comes_from_headers_then_peer() {
    let (app, _) = app_with(Config { rate_limit: 1, ..Config::default() });
    create_room(&app, ROOM, OWNER).await;
    let path = format!("/rooms/{ROOM}/offers");
    let post = |headers: Vec<(&'static str, &'static str)>, peer: Option<&'static str>| {
        let app = app.clone();
        let path = path.clone();
        async move {
            let mut req = request("POST", &path, &headers, r#"{"msg":"x"}"#);
            if let Some(p) = peer {
                req.extensions_mut().insert(ConnectInfo(p.parse::<SocketAddr>().unwrap()));
            }
            run(&app, req).await.status
        }
    };
    // Освобождаем счётчик создания комнаты (он общий): считаем только `post`
    // с новыми адресами, каждый раз — первый запрос адреса.
    let created = StatusCode::CREATED;
    let limited = StatusCode::TOO_MANY_REQUESTS;

    // X-Real-IP важнее X-Forwarded-For.
    let real = vec![("x-real-ip", "198.51.100.1"), ("x-forwarded-for", "198.51.100.9")];
    assert_eq!(post(real.clone(), None).await, created);
    assert_eq!(post(real, None).await, limited);
    // По-прежнему тот же адрес по X-Real-IP, хоть X-Forwarded-For другой.
    let same = vec![("x-real-ip", "198.51.100.1"), ("x-forwarded-for", "198.51.100.77")];
    assert_eq!(post(same, None).await, limited);

    // Без X-Real-IP берётся первый адрес X-Forwarded-For.
    let xff = vec![("x-forwarded-for", "198.51.100.2, 10.0.0.1")];
    assert_eq!(post(xff.clone(), None).await, created);
    assert_eq!(post(xff, None).await, limited);
    // Цепочка с тем же первым адресом — тот же клиент.
    let xff2 = vec![("x-forwarded-for", "198.51.100.2,10.9.9.9")];
    assert_eq!(post(xff2, None).await, limited);

    // Мусор в заголовках игнорируется, остаётся адрес соединения.
    let junk = vec![("x-real-ip", "не-адрес"), ("x-forwarded-for", "тоже-нет")];
    assert_eq!(post(junk.clone(), Some("198.51.100.3:5555")).await, created);
    assert_eq!(post(junk, Some("198.51.100.3:6666")).await, limited);

    // IPv6: один клиент — вся сеть /64.
    let v6a = vec![("x-real-ip", "2001:db8:1:2::1")];
    let v6b = vec![("x-real-ip", "2001:db8:1:2:ffff::9")];
    assert_eq!(post(v6a, None).await, created);
    assert_eq!(post(v6b, None).await, limited);
}

#[tokio::test]
async fn room_limit_is_503() {
    let (app, _) = app_with(Config { max_rooms: 2, ..Config::default() });
    assert_eq!(create_room(&app, "room-aaaaaaaa", OWNER).await.status, StatusCode::OK);
    assert_eq!(create_room(&app, "room-bbbbbbbb", OWNER).await.status, StatusCode::OK);
    assert_eq!(create_room(&app, "room-cccccccc", OWNER).await.status, StatusCode::SERVICE_UNAVAILABLE);
    // Продление существующей при полном списке работает.
    assert_eq!(create_room(&app, "room-aaaaaaaa", OWNER).await.status, StatusCode::OK);
    // Освободили место — новая комната создаётся.
    let h = [("x-owner", OWNER)];
    send(&app, "DELETE", "/rooms/room-aaaaaaaa", &h, "").await;
    assert_eq!(create_room(&app, "room-cccccccc", OWNER).await.status, StatusCode::OK);
}

#[tokio::test(start_paused = true)]
async fn storage_budget_is_503_and_freed_with_room() {
    let (app, _) = app_with(Config { max_bytes: 100, ..Config::default() });
    create_room(&app, ROOM, OWNER).await;
    let sid = offer_sid(&app, ROOM, &"a".repeat(60)).await;
    assert_eq!(post_offer(&app, ROOM, &"b".repeat(60)).await.status, StatusCode::SERVICE_UNAVAILABLE);
    // Ответ тоже учитывается; замена прежнего ответа место не удваивает.
    assert_eq!(put_answer(&app, ROOM, &sid, OWNER, &"c".repeat(40)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(put_answer(&app, ROOM, &sid, OWNER, &"d".repeat(40)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(
        put_answer(&app, ROOM, &sid, OWNER, &"e".repeat(41)).await.status,
        StatusCode::SERVICE_UNAVAILABLE
    );
    // Закрыли комнату — место вернулось.
    let h = [("x-owner", OWNER)];
    send(&app, "DELETE", &format!("/rooms/{ROOM}"), &h, "").await;
    create_room(&app, ROOM, OWNER).await;
    assert_eq!(post_offer(&app, ROOM, &"f".repeat(100)).await.status, StatusCode::CREATED);
}

#[tokio::test(start_paused = true)]
async fn expired_room_is_404_immediately_and_swept() {
    let (app, state) = app();
    let h = [("x-owner", OWNER)];
    send(&app, "PUT", &format!("/rooms/{ROOM}"), &h, r#"{"ttl":60}"#).await;
    let sid = offer_sid(&app, ROOM, "offer").await;

    advance(Duration::from_secs(59)).await;
    assert_eq!(get_offers(&app, ROOM, OWNER, "wait=0").await.status, StatusCode::OK);
    // Продление сдвигает срок от момента последнего PUT.
    send(&app, "PUT", &format!("/rooms/{ROOM}"), &h, r#"{"ttl":60}"#).await;
    advance(Duration::from_secs(59)).await;
    assert_eq!(post_offer(&app, ROOM, "x").await.status, StatusCode::CREATED);

    advance(Duration::from_secs(2)).await;
    // Уборщик ещё не приходил, но комната уже «не существует» для всех запросов.
    assert_eq!(state.rooms(), 1);
    assert_eq!(post_offer(&app, ROOM, "x").await.status, StatusCode::NOT_FOUND);
    assert_eq!(get_offers(&app, ROOM, OWNER, "wait=0").await.status, StatusCode::NOT_FOUND);
    assert_eq!(get_answer(&app, ROOM, &sid, "wait=0").await.status, StatusCode::NOT_FOUND);
    assert_eq!(put_answer(&app, ROOM, &sid, OWNER, "x").await.status, StatusCode::NOT_FOUND);
    assert_eq!(state.rooms(), 0);

    // На месте истёкшей комнаты может встать другой владелец (не 409), без старых предложений.
    assert_eq!(create_room(&app, ROOM, OTHER).await.status, StatusCode::OK);
    assert_eq!(get_offers(&app, ROOM, OTHER, "wait=0").await.json(), json!({"offers": []}));
}

#[tokio::test(start_paused = true)]
async fn sweeper_removes_expired_rooms() {
    let (app, state) = app();
    let h = [("x-owner", OWNER)];
    send(&app, "PUT", "/rooms/room-short1", &h, r#"{"ttl":60}"#).await;
    send(&app, "PUT", "/rooms/room-long22", &h, r#"{"ttl":600}"#).await;
    advance(Duration::from_secs(61)).await;
    assert_eq!(state.rooms(), 2);
    state.sweep();
    assert_eq!(state.rooms(), 1);
}

#[tokio::test(start_paused = true)]
async fn waiting_poll_ends_with_404_when_room_expires_or_is_deleted() {
    let (app, _) = app();
    let h = [("x-owner", OWNER)];
    send(&app, "PUT", &format!("/rooms/{ROOM}"), &h, r#"{"ttl":60}"#).await;

    // Комната истекает посреди ожидания.
    advance(Duration::from_secs(50)).await;
    let t = Instant::now();
    let poll = tokio::spawn({
        let app = app.clone();
        async move { get_offers(&app, ROOM, OWNER, "wait=30").await }
    });
    assert_eq!(poll.await.unwrap().status, StatusCode::NOT_FOUND);
    assert!(t.elapsed() < Duration::from_secs(11));

    // Владелец закрывает комнату — ждущие узнают сразу.
    create_room(&app, ROOM, OWNER).await;
    let sid = offer_sid(&app, ROOM, "offer").await;
    let poll = tokio::spawn({
        let app = app.clone();
        async move { get_answer(&app, ROOM, &sid, "wait=30").await }
    });
    sleep(Duration::from_millis(50)).await;
    let t = Instant::now();
    send(&app, "DELETE", &format!("/rooms/{ROOM}"), &h, "").await;
    assert_eq!(poll.await.unwrap().status, StatusCode::NOT_FOUND);
    assert!(t.elapsed() < Duration::from_secs(1));
}

#[tokio::test(start_paused = true)]
async fn shutdown_releases_waiting_polls() {
    let (app, state) = app();
    create_room(&app, ROOM, OWNER).await;
    let poll = tokio::spawn({
        let app = app.clone();
        async move { get_offers(&app, ROOM, OWNER, "wait=30").await }
    });
    sleep(Duration::from_millis(50)).await;
    state.close();
    let r = poll.await.unwrap();
    assert_eq!(r.json(), json!({"offers": []}));
    assert!(Instant::now().elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn unknown_route_and_method() {
    let (app, _) = app();
    assert_eq!(send(&app, "GET", "/nope", &[], "").await.status, StatusCode::NOT_FOUND);
    assert_eq!(send(&app, "POST", "/health", &[], "").await.status, StatusCode::METHOD_NOT_ALLOWED);
    let r = run(&app, Request::get("/health").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "без префикса /share/api/v1 маршрутов нет");
}
