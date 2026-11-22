use super::*;
use std::time::Duration;

#[test]
fn persists_pause_and_replaces_settings_atomically() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let settings_path = directory.path().join("data/settings.json");
    let defaults = Settings::defaults(None);
    defaults.save(&settings_path)?;
    let paused = Settings {
        preferences: Preferences {
            poll_interval_seconds: 120,
            notify_existing_on_first_run: false,
            start_with_windows: false,
            filter: StreamFilter::default(),
        },
        enabled: false,
        ..defaults.clone()
    };
    paused.save(&settings_path)?;
    let reopened = Settings::load(&settings_path, defaults);
    assert_eq!(paused.preferences, reopened.preferences);
    assert!(!reopened.enabled);
    Ok(())
}

#[test]
fn check_now_rejects_retry_cooldown_and_can_resume_afterward() {
    let defaults = Settings::defaults(None);
    let (control, mut receiver) = Control::new(
        PathBuf::new(),
        defaults,
        Some(Twitch::new(
            http::client().unwrap(),
            String::new(),
            String::new(),
        )),
        Some(PathBuf::new()),
    );
    control.lock_status().retry_not_before = Some(Instant::now() + Duration::from_secs(60));

    assert!(matches!(
        control.check_now(),
        Err(ControlError::Unavailable(RETRY_MESSAGE))
    ));
    assert_eq!(receiver.try_recv(), Err(mpsc::error::TryRecvError::Empty));

    control.lock_status().retry_not_before = Some(Instant::now());
    assert!(control.check_now().is_ok());
    assert_eq!(receiver.try_recv(), Ok(()));
}
#[test]
fn legacy_settings_keep_pause_and_explicit_first_run_opt_in() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("settings.json");
    std::fs::write(
        &path,
        r#"{"enabled":false,"preferences":{"poll_interval_seconds":180,"start_with_windows":true,"notify_existing_on_first_run":true}}"#,
    )?;
    let settings = Settings::load(&path, Settings::defaults(None));
    assert!(!settings.enabled);
    assert!(settings.preferences.notify_existing_on_first_run);
    assert!(settings.preferences.start_with_windows);
    assert_eq!(settings.preferences.poll_interval_seconds, 180);
    assert_eq!(settings.preferences.filter, StreamFilter::default());
    assert!(
        !Settings::defaults(None)
            .preferences
            .notify_existing_on_first_run
    );
    Ok(())
}

#[cfg(windows)]
#[tokio::test]
async fn validated_profile_reconnects_worker_without_restart_and_bad_replacement_is_atomic()
-> Result<()> {
    use crate::tests::{FakeNotifier, twitch_server};
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
    let dir = tempfile::tempdir()?;
    let server = twitch_server().await;
    Mock::given(path("/helix/streams"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data":[],"pagination":{}})),
        )
        .mount(&server)
        .await;
    let database_path = dir.path().join("data/alerts.sqlite3");
    let store = Store::open(&database_path)?;
    let settings_path = database_path.with_file_name("settings.json");
    let (control, checks) = Control::new(
        settings_path.clone(),
        Settings::defaults(None),
        None,
        Some(database_path),
    );
    let test = async {
        assert!(!control.lock_status().configured);
        assert_eq!(
            control.history(String::new(), 0, 50).await.unwrap().total,
            0
        );
        let candidate = Credentials {
            client_id: "test-client".into(),
            client_secret: "test-secret".into(),
        };
        control
            .install_credentials(candidate, Twitch::for_test(http::client()?, &server.uri()))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(4), async {
            while control.lock_status().last_checked_at.is_none() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await?;
        let file = settings_path.with_file_name("twitch.dpapi");
        let encrypted = std::fs::read(&file)?;
        assert!(!String::from_utf8_lossy(&encrypted).contains("test-secret"));
        let saved = Credentials::load(&file)?.unwrap();
        assert!(saved.client_id == "test-client" && saved.client_secret == "test-secret");
        let serialized = serde_json::to_string(&control.lock_status().clone())?;
        assert!(!serialized.contains("test-secret") && !serialized.contains("test-client"));
        let rejected = MockServer::start().await;
        Mock::given(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&rejected)
            .await;
        let replacement = Credentials {
            client_id: "bad".into(),
            client_secret: "bad-secret".into(),
        };
        let result = control
            .install_credentials(
                replacement,
                Twitch::for_test(http::client()?, &rejected.uri()),
            )
            .await;
        assert!(matches!(result, Err(ControlError::Invalid(_))));
        assert_eq!(std::fs::read(file)?, encrypted);
        assert!(control.lock_status().running);
        control.stop();
        Ok::<_, anyhow::Error>(())
    };
    tokio::try_join!(
        test,
        run_worker(store, FakeNotifier::default(), &control, checks)
    )?;
    Ok(())
}
#[tokio::test]
async fn changing_language_cancels_the_old_page_and_checks_the_new_filter_immediately() -> Result<()>
{
    use crate::tests::{FakeNotifier, twitch_server};
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{path, query_param},
    };
    let dir = tempfile::tempdir()?;
    let server = twitch_server().await;
    let mut settings = Settings::defaults(None);
    settings.preferences.notify_existing_on_first_run = true;
    for (id, language, delay) in [("old", "fr", 2000), ("new", "en", 0)] {
        let row = serde_json::json!({
            "id":id, "language":language, "game_id":"460636", "type":"live", "title":"test",
            "viewer_count":1,"started_at":"2026-11-21T12:00:00Z","user_name":"A","user_login":"channel"
        });
        Mock::given(path("/helix/streams"))
            .and(query_param("language", language))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(delay))
                    .set_body_json(serde_json::json!({"data":[row],"pagination":{}})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let database = dir.path().join("alerts.sqlite3");
    let store = Store::open(&database)?;
    let twitch = Twitch::for_test(http::client()?, &server.uri());
    let (control, checks) = Control::new(
        dir.path().join("settings.json"),
        settings.clone(),
        Some(twitch),
        Some(database),
    );
    let test = async {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if server
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .any(|r| r.url.path() == "/helix/streams")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            settings.preferences.filter.language = "en".into();
            control.apply(settings);
            while control.lock_status().last_checked_at.is_none() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await?;
        let history = control.history(String::new(), 0, 50).await.unwrap();
        assert_eq!(
            (history.total, history.items[0].stream_id.as_str()),
            (1, "new")
        );
        assert_eq!(control.lock_status().live_streams[0].id, "new");
        control.stop();
        Ok::<_, anyhow::Error>(())
    };
    tokio::try_join!(
        test,
        run_worker(store, FakeNotifier::default(), &control, checks)
    )?;
    Ok(())
}
