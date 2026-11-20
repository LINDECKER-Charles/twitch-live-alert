use crate::{
    http,
    notification::{NotificationError, Notifier, toast_xml},
    store::Store,
    twitch::{Stream, Twitch},
    watcher::Watcher,
};
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

#[derive(Clone, Default)]
struct FakeNotifier {
    attempts: Arc<Mutex<Vec<Stream>>>,
    failures: Arc<Mutex<VecDeque<bool>>>,
    delay: Duration,
}
impl Notifier for FakeNotifier {
    async fn notify(&self, stream: &Stream) -> anyhow::Result<()> {
        self.attempts.lock().unwrap().push(stream.clone());
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        if self.failures.lock().unwrap().pop_front().unwrap_or(false) {
            return Err(NotificationError("échec simulé".into()).into());
        }
        Ok(())
    }
}

fn watcher(server: &MockServer, dir: &TempDir, notify_existing: bool) -> Watcher<FakeNotifier> {
    Watcher {
        twitch: Twitch::for_test(http::client().unwrap(), &server.uri()),
        notifier: FakeNotifier::default(),
        store: Store::open(&dir.path().join("state.sqlite3")).unwrap(),
        notify_existing,
        last_streams: Vec::new(),
    }
}

#[tokio::test]
async fn pipeline_filters_language_and_game_and_follows_empty_pages() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    let mut english = stream("2");
    english["language"] = json!("en");
    english["title"] = json!("[FR] Bloodborne");
    let mut other = stream("3");
    other["game_id"] = json!("other");
    let mut offline = stream("4");
    offline["type"] = json!("");
    streams_sequence(
        &server,
        vec![
            ResponseTemplate::new(200).set_body_json(
                json!({"data":[stream("1"),english,other,offline],"pagination":{"cursor":"next"}}),
            ),
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[],"pagination":{"cursor":"last"}})),
            page(vec![stream("1"), stream("5")]),
        ],
    )
    .await;
    let mut watcher = watcher(&server, &dir, true);
    assert_eq!(watcher.cycle().await.unwrap(), 2);
    {
        let attempts = watcher.notifier.attempts.lock().unwrap();
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].url(), "https://www.twitch.tv/hunter_1");
    }
    assert!(!watcher.store.contains("2").unwrap());
    assert_eq!(watcher.last_streams.len(), 2);
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
    let mut first = watcher(&server, &dir, true);
    assert_eq!(first.cycle().await.unwrap(), 1);
    drop(first);
    let mut restarted = watcher(&server, &dir, true);
    assert_eq!(restarted.cycle().await.unwrap(), 0);
    assert_eq!(restarted.cycle().await.unwrap(), 0);
    assert_eq!(restarted.cycle().await.unwrap(), 1);
    assert_eq!(restarted.notifier.attempts.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn detects_category_entry_without_repeating_title_changes() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    let mut renamed = stream("1");
    renamed["title"] = json!("Nouveau titre");
    streams_sequence(
        &server,
        vec![page(vec![]), page(vec![stream("1")]), page(vec![renamed])],
    )
    .await;
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
    let mut first = watcher(&server, &dir, false);
    assert_eq!(first.cycle().await.unwrap(), 0);
    drop(first);
    let mut restarted = watcher(&server, &dir, false);
    assert_eq!(restarted.cycle().await.unwrap(), 1);
    assert_eq!(restarted.notifier.attempts.lock().unwrap()[0].id, "2");
}

#[tokio::test]
async fn failed_notification_retries_without_repeating_successes() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    streams_sequence(&server, vec![page(vec![stream("1"), stream("2")])]).await;
    let mut watcher = watcher(&server, &dir, true);
    *watcher.notifier.failures.lock().unwrap() = VecDeque::from([false, true, false]);
    assert!(watcher.cycle().await.is_err());
    assert!(watcher.store.contains("1").unwrap());
    assert!(!watcher.store.contains("2").unwrap());
    assert_eq!(watcher.cycle().await.unwrap(), 1);
    assert_eq!(
        watcher
            .notifier
            .attempts
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "2"]
    );
}

#[tokio::test]
async fn incomplete_snapshot_never_initializes_baseline() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    streams_sequence(
        &server,
        vec![
            ResponseTemplate::new(200)
                .set_body_json(json!({"data":[stream("1")],"pagination":{"cursor":"next"}})),
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
    assert!(watcher.notifier.attempts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn repeated_cursor_does_not_touch_state() {
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
    assert!(watcher.notifier.attempts.lock().unwrap().is_empty());
}

#[tokio::test]
async fn shutdown_finishes_current_notification_and_persists_it() {
    let server = twitch_server().await;
    let dir = TempDir::new().unwrap();
    streams_sequence(&server, vec![page(vec![stream("1"), stream("2")])]).await;
    let mut watcher = watcher(&server, &dir, true);
    watcher.notifier.delay = Duration::from_millis(100);
    let attempts = watcher.notifier.attempts.clone();
    let (sender, mut receiver) = tokio::sync::watch::channel(false);
    let stop = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(5), async {
            while attempts.lock().unwrap().is_empty() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        sender.send(true).unwrap();
    });
    assert_eq!(watcher.cycle_with_shutdown(&mut receiver).await.unwrap(), 1);
    stop.await.unwrap();
    assert!(watcher.store.contains("1").unwrap());
    assert!(!watcher.store.contains("2").unwrap());
    assert_eq!(watcher.cycle().await.unwrap(), 1);
}

#[test]
fn native_toast_escapes_untrusted_text_and_opens_twitch_by_protocol() {
    let xml = toast_xml(
        "A < B & C",
        "</text><action/>\0 🩸",
        "https://www.twitch.tv/hunter_1",
    );
    assert!(xml.contains("A &lt; B &amp; C"));
    assert!(xml.contains("&lt;/text&gt;&lt;action/&gt;"));
    assert!(!xml.contains('\0'));
    assert!(xml.contains("activationType=\"protocol\" launch=\"https://www.twitch.tv/hunter_1\""));
    assert!(xml.contains("content=\"Voir sur Twitch\""));
    assert!(toast_xml("T", &"🩸".repeat(20000), "https://www.twitch.tv/test").len() < 4000);
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
async fn transport_errors_do_not_expose_request_path() {
    let error = http::client()
        .unwrap()
        .post("http://127.0.0.1:1/private-secret")
        .send()
        .await
        .unwrap_err();
    let safe = http::transport(error, "Twitch");
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
