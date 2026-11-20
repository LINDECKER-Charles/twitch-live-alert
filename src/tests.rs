use crate::{
    discord::{self, Discord},
    http,
    store::Store,
    twitch::Twitch,
    watcher::Watcher,
};
use reqwest::Url;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;
use wiremock::{
    Mock, MockServer, Request, ResponseTemplate,
    matchers::{header, method, path, query_param},
};

fn stream(id: &str) -> Value {
    json!({
        "id": id, "user_login": format!("hunter_{id}"), "user_name": "Chasseur",
        "game_id": "460636", "language": "fr", "type": "live", "title": "Découverte de Yharnam",
        "started_at": "2026-11-20T12:00:00Z", "viewer_count": 5
    })
}

fn page(streams: Vec<Value>) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"data": streams, "pagination": {}}))
}

async fn twitch_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token":"test-access-token", "expires_in": 10000})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/oauth2/validate"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"client_id": "test-client", "expires_in": 10000})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/helix/games"))
        .and(query_param("name", "Bloodborne"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": [{"id":"460636", "name":"Bloodborne"}]})),
        )
        .mount(&server)
        .await;
    server
}

async fn streams_sequence(server: &MockServer, responses: Vec<ResponseTemplate>) {
    let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
    Mock::given(method("GET"))
        .and(path("/helix/streams"))
        .and(header("Client-Id", "test-client"))
        .and(header("Authorization", "Bearer test-access-token"))
        .and(query_param("game_id", "460636"))
        .and(query_param("language", "fr"))
        .and(query_param("type", "live"))
        .and(query_param("first", "100"))
        .respond_with(move |_: &Request| {
            let mut responses = responses.lock().unwrap();
            if responses.len() > 1 {
                responses.pop_front().unwrap()
            } else {
                responses.front().unwrap().clone()
            }
        })
        .mount(server)
        .await;
}

async fn discord_sequence(server: &MockServer, responses: Vec<ResponseTemplate>) {
    let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
    Mock::given(method("POST"))
        .and(path("/webhook"))
        .and(query_param("wait", "true"))
        .respond_with(move |_: &Request| {
            let mut responses = responses.lock().unwrap();
            if responses.len() > 1 {
                responses.pop_front().unwrap()
            } else {
                responses.front().unwrap().clone()
            }
        })
        .mount(server)
        .await;
}

fn confirmed() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"id":"discord-message"}))
}

fn watcher(server: &MockServer, dir: &TempDir, notify_existing: bool) -> Watcher {
    let client = http::client().unwrap();
    Watcher {
        twitch: Twitch::for_test(client.clone(), &server.uri()),
        discord: Discord::new(
            client,
            Url::parse(&format!("{}/webhook", server.uri())).unwrap(),
        ),
        store: Store::open(&dir.path().join("state.sqlite3")).unwrap(),
        notify_existing,
    }
}

async fn notifications(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/webhook")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

#[tokio::test]
async fn full_pipeline_filters_french_and_game_and_follows_short_pages() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    let mut english = stream("2");
    english["language"] = json!("en");
    english["title"] = json!("[FR] Bloodborne");
    let mut other_game = stream("3");
    other_game["game_id"] = json!("other");
    let mut offline = stream("4");
    offline["type"] = json!("");
    streams_sequence(&server, vec![
        ResponseTemplate::new(200).set_body_json(json!({"data": [stream("1"), english, other_game, offline], "pagination": {"cursor":"next"}})),
        ResponseTemplate::new(200).set_body_json(json!({"data": [], "pagination": {"cursor":"last"}})),
        page(vec![stream("1"), stream("5")]),
    ]).await;
    discord_sequence(&server, vec![confirmed()]).await;
    let mut watcher = watcher(&server, &dir, true);
    assert_eq!(watcher.cycle().await.unwrap(), 2);
    assert!(watcher.store.contains("1").unwrap());
    assert!(!watcher.store.contains("2").unwrap());
    let messages = notifications(&server).await;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["allowed_mentions"]["parse"], json!([]));
    assert_eq!(
        messages[0]["embeds"][0]["url"],
        "https://www.twitch.tv/hunter_1"
    );
    let requests = server.received_requests().await.unwrap();
    assert!(requests.iter().any(|r| {
        r.url
            .query_pairs()
            .any(|(k, v)| k == "after" && v == "next")
    }));
    assert!(requests.iter().any(|r| {
        r.url
            .query_pairs()
            .any(|(k, v)| k == "after" && v == "last")
    }));
    let token = requests
        .iter()
        .find(|r| r.url.path() == "/oauth2/token")
        .unwrap();
    assert!(token.url.query().is_none());
    assert!(String::from_utf8_lossy(&token.body).contains("client_secret=test-secret"));
}

#[tokio::test]
async fn notifications_survive_restart_and_temporary_disappearance() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    streams_sequence(
        &server,
        vec![
            page(vec![stream("1")]),
            page(vec![]),
            page(vec![stream("1")]),
            page(vec![stream("2")]),
        ],
    )
    .await;
    discord_sequence(&server, vec![confirmed()]).await;
    let mut first = watcher(&server, &dir, true);
    assert_eq!(first.cycle().await.unwrap(), 1);
    drop(first);
    let mut restarted = watcher(&server, &dir, true);
    assert_eq!(restarted.cycle().await.unwrap(), 0);
    assert_eq!(restarted.cycle().await.unwrap(), 0);
    assert_eq!(restarted.cycle().await.unwrap(), 1);
    assert_eq!(notifications(&server).await.len(), 2);
}

#[tokio::test]
async fn detects_entry_into_category_and_suppresses_title_changes() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    let mut renamed = stream("1");
    renamed["title"] = json!("Nouveau titre");
    streams_sequence(
        &server,
        vec![page(vec![]), page(vec![stream("1")]), page(vec![renamed])],
    )
    .await;
    discord_sequence(&server, vec![confirmed()]).await;
    let mut watcher = watcher(&server, &dir, true);
    assert_eq!(watcher.cycle().await.unwrap(), 0);
    assert_eq!(watcher.cycle().await.unwrap(), 1);
    assert_eq!(watcher.cycle().await.unwrap(), 0);
}

#[tokio::test]
async fn baseline_is_only_taken_once_and_persists() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    streams_sequence(
        &server,
        vec![
            page(vec![stream("1")]),
            page(vec![stream("1"), stream("2")]),
        ],
    )
    .await;
    discord_sequence(&server, vec![confirmed()]).await;
    let mut first = watcher(&server, &dir, false);
    assert_eq!(first.cycle().await.unwrap(), 0);
    assert!(first.store.contains("1").unwrap());
    drop(first);
    let mut restarted = watcher(&server, &dir, false);
    assert_eq!(restarted.cycle().await.unwrap(), 1);
    assert_eq!(notifications(&server).await.len(), 1);
}

#[tokio::test]
async fn failed_delivery_retries_without_repeating_successes() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    streams_sequence(&server, vec![page(vec![stream("1"), stream("2")])]).await;
    discord_sequence(
        &server,
        vec![confirmed(), ResponseTemplate::new(503), confirmed()],
    )
    .await;
    let mut watcher = watcher(&server, &dir, true);
    assert!(watcher.cycle().await.is_err());
    assert!(watcher.store.contains("1").unwrap());
    assert!(!watcher.store.contains("2").unwrap());
    assert_eq!(watcher.cycle().await.unwrap(), 1);
    let messages = notifications(&server).await;
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages[1]["embeds"][0]["url"],
        messages[2]["embeds"][0]["url"]
    );
}

#[tokio::test]
async fn discord_rate_limit_is_respected_and_not_recorded_as_sent() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    streams_sequence(&server, vec![page(vec![stream("1"), stream("2")])]).await;
    discord_sequence(
        &server,
        vec![ResponseTemplate::new(429).set_body_json(json!({"retry_after":120.5}))],
    )
    .await;
    let mut watcher = watcher(&server, &dir, true);
    let error = watcher.cycle().await.unwrap_err();
    assert_eq!(
        http::retry_delay(&error, 1, Duration::from_secs(60)),
        Duration::from_secs_f64(120.5)
    );
    assert!(!watcher.store.contains("1").unwrap());
    assert_eq!(notifications(&server).await.len(), 1);
}

#[tokio::test]
async fn discord_requires_message_confirmation_even_on_success_status() {
    for response in [
        ResponseTemplate::new(204),
        ResponseTemplate::new(200).set_body_json(json!({"id":""})),
    ] {
        let server = twitch_server().await;
        let dir = TempDir::new().unwrap();
        streams_sequence(&server, vec![page(vec![stream("1")])]).await;
        discord_sequence(&server, vec![response]).await;
        let mut watcher = watcher(&server, &dir, true);
        assert!(watcher.cycle().await.is_err());
        assert!(!watcher.store.contains("1").unwrap());
    }
}

#[tokio::test]
async fn incomplete_snapshot_never_initializes_baseline() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    streams_sequence(
        &server,
        vec![
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[stream("1")], "pagination":{"cursor":"next"}})),
            ResponseTemplate::new(500),
            page(vec![stream("1"), stream("2")]),
        ],
    )
    .await;
    let mut watcher = watcher(&server, &dir, false);
    assert!(watcher.cycle().await.is_err());
    assert!(!watcher.store.contains("1").unwrap());
    assert_eq!(watcher.cycle().await.unwrap(), 0);
    assert!(watcher.store.contains("2").unwrap());
    assert!(notifications(&server).await.is_empty());
}

#[tokio::test]
async fn repeated_cursor_fails_without_partial_state() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    streams_sequence(
        &server,
        vec![
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[stream("1")],"pagination":{"cursor":"loop"}})),
        ],
    )
    .await;
    let mut watcher = watcher(&server, &dir, true);
    assert!(
        watcher
            .cycle()
            .await
            .unwrap_err()
            .to_string()
            .contains("pagination")
    );
    assert!(!watcher.store.contains("1").unwrap());
    assert!(notifications(&server).await.is_empty());
}

#[tokio::test]
async fn unauthorized_streams_refreshes_token_once() {
    let server = twitch_server().await;
    streams_sequence(
        &server,
        vec![ResponseTemplate::new(401), page(vec![stream("1")])],
    )
    .await;
    let mut twitch = Twitch::for_test(http::client().unwrap(), &server.uri());
    assert_eq!(twitch.french_bloodborne_streams().await.unwrap().len(), 1);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/oauth2/token")
            .count(),
        2
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/oauth2/validate")
            .count(),
        2
    );
}

#[tokio::test]
async fn persistent_unauthorized_is_bounded() {
    let server = twitch_server().await;
    streams_sequence(&server, vec![ResponseTemplate::new(401)]).await;
    let mut twitch = Twitch::for_test(http::client().unwrap(), &server.uri());
    assert!(twitch.french_bloodborne_streams().await.is_err());
    let requests = server.received_requests().await.unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/helix/streams")
            .count(),
        2
    );
}

#[tokio::test]
async fn revoked_validation_refreshes_before_any_stream_query() {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    Mock::given(path("/oauth2/token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"access_token":"test", "expires_in":10000})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/oauth2/validate"))
        .respond_with(move |_: &Request| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(401)
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"client_id":"test-client","expires_in":10000}))
            }
        })
        .mount(&server)
        .await;
    Mock::given(path("/helix/games"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[{"id":"460636","name":"Bloodborne"}]})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/helix/streams"))
        .respond_with(page(vec![]))
        .mount(&server)
        .await;
    let mut twitch = Twitch::for_test(http::client().unwrap(), &server.uri());
    assert!(twitch.french_bloodborne_streams().await.unwrap().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn malformed_and_http_error_bodies_never_expose_secrets() {
    for response in [
        ResponseTemplate::new(200).set_body_string("private-secret"),
        ResponseTemplate::new(403).set_body_string("private-secret"),
    ] {
        let server = twitch_server().await;
        streams_sequence(&server, vec![response]).await;
        let mut twitch = Twitch::for_test(http::client().unwrap(), &server.uri());
        let error = twitch.french_bloodborne_streams().await.unwrap_err();
        assert!(!format!("{error:?} {error:#}").contains("private-secret"));
    }
}

#[tokio::test]
async fn transport_errors_do_not_expose_webhook_path() {
    let error = http::client()
        .unwrap()
        .post("http://127.0.0.1:1/private-secret")
        .send()
        .await
        .unwrap_err();
    let safe = http::transport(error, "Discord");
    assert!(!format!("{safe:?} {safe:#}").contains("private-secret"));
}

#[test]
fn sqlite_prevents_two_instances_and_rejects_corruption() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.sqlite3");
    let store = Store::open(&path).unwrap();
    assert!(Store::open(&path).is_err());
    drop(store);
    assert!(Store::open(&path).is_ok());
    let broken = dir.path().join("broken.sqlite3");
    std::fs::write(&broken, b"invalid sqlite data").unwrap();
    assert!(Store::open(&broken).is_err());
}

#[test]
fn discord_payload_neutralizes_mentions_and_limits_unicode() {
    let mut value = stream("1");
    value["title"] = json!(format!("@everyone **hello** {}", "🩸".repeat(5000)));
    let stream = serde_json::from_value(value).unwrap();
    let payload = discord::payload(&stream);
    assert_eq!(payload["allowed_mentions"]["parse"], json!([]));
    let description = payload["embeds"][0]["description"].as_str().unwrap();
    assert!(description.contains("\\*\\*hello\\*\\*"));
    assert!(description.encode_utf16().count() <= 2000);
}

#[test]
fn exponential_backoff_is_bounded_without_ignoring_poll_interval() {
    let error = anyhow::anyhow!("test");
    assert_eq!(
        http::retry_delay(&error, 1, Duration::from_secs(60)),
        Duration::from_secs(60)
    );
    assert_eq!(
        http::retry_delay(&error, u32::MAX, Duration::from_secs(60)),
        Duration::from_secs(320)
    );
}

#[tokio::test]
async fn graceful_stop_finishes_inflight_delivery_and_stops_before_next() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    streams_sequence(&server, vec![page(vec![stream("1"), stream("2")])]).await;
    let started = Arc::new(AtomicUsize::new(0));
    let count = started.clone();
    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(move |_: &Request| {
            count.fetch_add(1, Ordering::SeqCst);
            confirmed().set_delay(Duration::from_millis(100))
        })
        .mount(&server)
        .await;
    let (sender, mut receiver) = tokio::sync::watch::channel(false);
    let stop = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(5), async {
            while started.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        sender.send(true).unwrap();
    });
    let mut watcher = watcher(&server, &dir, true);
    assert_eq!(watcher.cycle_with_shutdown(&mut receiver).await.unwrap(), 1);
    stop.await.unwrap();
    assert!(watcher.store.contains("1").unwrap());
    assert!(!watcher.store.contains("2").unwrap());
    assert_eq!(notifications(&server).await.len(), 1);
    assert_eq!(watcher.cycle().await.unwrap(), 1);
    assert_eq!(notifications(&server).await.len(), 2);
}

#[tokio::test]
async fn failed_replacement_validation_is_not_skipped_on_next_cycle() {
    let server = twitch_server().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    Mock::given(path("/oauth2/validate"))
        .respond_with(
            move |_: &Request| match count.fetch_add(1, Ordering::SeqCst) {
                0 => ResponseTemplate::new(401),
                1 => ResponseTemplate::new(503),
                _ => ResponseTemplate::new(200)
                    .set_body_json(json!({"client_id":"test-client", "expires_in":10000})),
            },
        )
        .with_priority(1)
        .mount(&server)
        .await;
    streams_sequence(&server, vec![page(vec![])]).await;
    let mut twitch = Twitch::for_test(http::client().unwrap(), &server.uri());
    assert!(twitch.french_bloodborne_streams().await.is_err());
    let requests = server.received_requests().await.unwrap();
    assert!(!requests.iter().any(|r| r.url.path().starts_with("/helix/")));
    assert!(twitch.french_bloodborne_streams().await.unwrap().is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}
