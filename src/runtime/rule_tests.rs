use super::*;
use crate::{
    rules::{LEGACY_ID, Rule},
    tests::{FakeNotifier, twitch_server},
};
use chrono::{TimeZone, Utc};
use rusqlite::{Connection, params};
use serde_json::json;
use wiremock::{Mock, ResponseTemplate, matchers::path};

fn rule(id: &str) -> Rule {
    let mut rule = Rule::legacy(StreamFilter::default());
    rule.id = id.into();
    rule.name = format!("Règle {id}");
    rule
}

fn stream_json() -> serde_json::Value {
    json!({"id":"s1","user_login":"hunter","user_name":"Chasseur","game_id":"460636","game_name":"Bloodborne",
        "language":"fr","type":"live","title":"Test","started_at":"2026-11-21T10:00:00Z","viewer_count":3})
}

#[test]
fn closed_and_disabled_rules_do_not_schedule_twitch_and_matching_filters_share_a_query()
-> Result<()> {
    let now = Utc.with_ymd_and_hms(2026, 11, 23, 3, 0, 0).unwrap(); // Monday
    let mut overnight = rule("overnight");
    overnight.detection = serde_json::from_value(
        json!({"timezone":"UTC","windows":[{"days":[7],"start":"22:00","end":"04:00"}]}),
    )?;
    let mut paused = rule("paused");
    paused.enabled = false;
    let mut closed = rule("closed");
    closed.detection = serde_json::from_value(
        json!({"timezone":"UTC","windows":[{"days":[1],"start":"08:00","end":"18:00"}]}),
    )?;
    let rules = vec![rule("all"), overnight, paused, closed];
    let groups = poll::due_groups(&rules, now);
    let before_opening = Utc.with_ymd_and_hms(2026, 11, 23, 7, 59, 45).unwrap();
    assert_eq!(
        poll::next_delay(&rules, before_opening, std::time::Duration::from_secs(3600)),
        std::time::Duration::from_secs(15)
    );
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0]
            .1
            .iter()
            .map(|rule| rule.id.as_str())
            .collect::<Vec<_>>(),
        vec!["all", "overnight"]
    );
    Ok(())
}

#[cfg(windows)]
#[test]
fn v020_migrates_thirteen_history_rows_filter_pause_dedup_and_preserves_profile_credentials()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = dir.path().join("alerts.sqlite3");
    let settings_path = dir.path().join("settings.json");
    std::fs::write(
        &settings_path,
        r#"{"preferences":{"category":"Dark Souls","language":"de","poll_interval_seconds":30,"start_with_windows":false,"notify_existing_on_first_run":false},"enabled":false}"#,
    )?;
    let pair_path = dir.path().join("twitch.dpapi");
    let synthetic = Credentials {
        client_id: "synthetic-fixture-id".into(),
        client_secret: "synthetic-fixture-pair".into(),
    };
    synthetic.save(&pair_path)?;
    let protected = std::fs::read(&pair_path)?;
    let old = Connection::open(&db)?;
    old.execute_batch("CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE deliveries(stream_id TEXT PRIMARY KEY,disposition TEXT NOT NULL,message_id TEXT,recorded_at INTEGER NOT NULL,user_name TEXT,title TEXT,url TEXT);
        CREATE TABLE baselines(filter_key TEXT NOT NULL,stream_id TEXT NOT NULL,PRIMARY KEY(filter_key,stream_id));
        INSERT INTO metadata VALUES ('initialized:de:dark souls','true');
        INSERT INTO baselines VALUES ('initialized:de:dark souls','initial');
        PRAGMA user_version=3;")?;
    for i in 1..=13 {
        old.execute("INSERT INTO deliveries VALUES(?1,'sent',NULL,?2,'Chaîne',?3,'https://www.twitch.tv/chaine')",
            params![format!("stream-{i}"),1000+i,format!("Titre {i}")])?;
    }
    drop(old);
    let settings = Settings::load(&settings_path, Settings::defaults(None));
    let rules = settings.rules();
    assert!(!settings.enabled);
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, LEGACY_ID);
    assert_eq!(rules[0].filter.category, "Dark Souls");
    assert_eq!(rules[0].filter.language, "de");
    for _ in 0..2 {
        let store = Store::open(&db)?;
        let rows = Store::history_at(&db, "", 0, 50)?;
        assert_eq!(rows.total, 13);
        for (index, row) in rows.items.iter().rev().enumerate() {
            let n = index + 1;
            assert_eq!(row.rule_id, LEGACY_ID);
            assert_eq!(row.channel, "desktop");
            assert_eq!(row.title, format!("Titre {n}"));
            assert_eq!(row.stream_id, format!("stream-{n}"));
            assert_eq!(row.user_name, "Chaîne");
            assert_eq!(row.url, "https://www.twitch.tv/chaine");
            assert_eq!(row.recorded_at, 1000 + n as u64);
            assert!(store.contains(&row.stream_id, &rules[0].filter)?);
        }
        assert!(store.contains("initial", &rules[0].filter)?);
        assert_eq!(std::fs::read(&pair_path)?, protected);
        let restored = Credentials::load(&pair_path)?.unwrap();
        assert!(
            restored.client_id == synthetic.client_id
                && restored.client_secret == synthetic.client_secret
        );
    }
    Ok(())
}

#[tokio::test]
async fn repeated_snapshots_restart_and_overlapping_rules_queue_one_delivery_per_channel()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = dir.path().join("alerts.sqlite3");
    let server = twitch_server().await;
    Mock::given(path("/helix/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[stream_json()]})))
        .expect(2)
        .mount(&server)
        .await;
    let mut both = rule("both");
    both.channels.email = true;
    let desktop = rule("desktop");
    let rules = [both, desktop];
    let mut twitch = Twitch::for_test(http::client()?, &server.uri());
    let (_stop, mut cancel) = watch::channel(false);
    for _ in 0..2 {
        let mut store = Store::open(&db)?;
        let fake = FakeNotifier::default();
        let mut poller = poll::Poller {
            store: &mut store,
            notifier: &fake,
            twitch: &mut twitch,
            notify_existing: true,
        };
        let report = poller.poll(&rules, &mut cancel).await?;
        assert!(report.warning.is_none());
        assert_eq!(report.live.len(), 2);
        assert_eq!(report.live[0].rule_id, "both");
        let rows = Store::history_at(&db, "", 0, 5)?;
        assert_eq!(rows.total, 2);
        let reader = Connection::open(&db)?;
        assert_eq!(
            reader.query_row("SELECT count(*) FROM mail_events", [], |r| r
                .get::<_, i64>(0))?,
            1
        );
    }
    Ok(())
}

#[tokio::test]
async fn saved_rules_edits_enable_disable_deletion_and_reopen_are_distinct_from_migration()
-> Result<()> {
    let dir = tempfile::tempdir()?;
    let p = dir.path().join("settings.json");
    let _store = Store::open(&dir.path().join("db"))?;
    let srv = twitch_server().await;
    let (control, _) = Control::new(
        p.clone(),
        Settings::defaults(None),
        Some(Twitch::for_test(http::client()?, &srv.uri())),
        Some(dir.path().join("db")),
    );
    let mut new = rule("");
    new.channels.email = true;
    let rules = control.upsert_rule(new).await.unwrap();
    assert_eq!(rules.len(), 2);
    let mut edited = rules[1].clone();
    assert!(!edited.id.is_empty());
    edited.name = "Anglais".into();
    edited.filter.language = "en".into();
    edited.enabled = false;
    control.upsert_rule(edited.clone()).await.unwrap();
    assert_eq!(control.lock_status().active_rule_count, 1);
    assert_eq!(
        Settings::load(&p, Settings::defaults(None)).rules()[1],
        edited
    );
    control.delete_rule(LEGACY_ID).await.unwrap();
    assert!(!control.lock_status().running);
    control.delete_rule(&edited.id).await.unwrap();
    assert!(
        Settings::load(&p, Settings::defaults(None))
            .rules()
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn http_rule_and_connection_controls_reject_remote_writes_and_disconnect_persists()
-> Result<()> {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("settings.json");
    let server = twitch_server().await;
    let make_session = || Twitch::for_test(http::client().unwrap(), &server.uri());
    let (control, _) = Control::new(
        path.clone(),
        Settings::defaults(None),
        Some(make_session()),
        Some(dir.path().join("db")),
    );
    let app = web::router(control.clone(), 37219);
    let request = |url: &'static str, body: Option<serde_json::Value>, local: bool| {
        let mut builder = Request::builder()
            .uri(url)
            .header("host", "127.0.0.1:37219");
        let payload = if let Some(body) = body {
            builder = builder
                .method("POST")
                .header("content-type", "application/json")
                .header(
                    "origin",
                    if local {
                        "http://127.0.0.1:37219"
                    } else {
                        "https://invalid.example"
                    },
                )
                .header("x-twitch-live-alert", "local");
            Body::from(body.to_string())
        } else {
            Body::empty()
        };
        app.clone().oneshot(builder.body(payload).unwrap())
    };
    let json = |response: axum::response::Response| async {
        let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    };
    let before = json(request("/api/connection", None, true).await?).await;
    assert_eq!(before["state"], "unverified");
    let verified = json(request("/api/connection/verify", Some(json!({})), true).await?).await;
    assert_eq!(verified["state"], "connected");
    assert!(verified["verified_at"].is_number());
    let input = json!({"id":"","name":"Secondaire","enabled":true,"category":"Bloodborne","language":"fr","channels":{"desktop":true,"email":false}});
    let denied = request("/api/rules", Some(input.clone()), false).await?;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let created = json(request("/api/rules", Some(input), true).await?).await;
    assert_eq!(created["rules"].as_array().unwrap().len(), 2);
    assert_eq!(created["rules"][1]["category"], "Bloodborne");
    let settings = json!({"poll_interval_seconds":90,"notify_existing_on_first_run":false,"start_with_windows":startup_enabled().unwrap_or(false)});
    // Avoid changing the real startup preference, which is separate from the temporary profile.
    let result = request("/api/settings", Some(settings), true).await?;
    assert_eq!(result.status(), StatusCode::OK);
    let listed = json(request("/api/rules", None, true).await?).await;
    assert_eq!(listed, created);
    let disconnected =
        json(request("/api/connection/disconnect", Some(json!({})), true).await?).await;
    assert_eq!(disconnected["state"], "unconfigured");
    assert!(!control.lock_status().running);
    let saved = Settings::load(&path, Settings::defaults(None));
    let (reopened, _) = Control::new(path, saved, Some(make_session()), None);
    assert_eq!(
        serde_json::to_value(reopened.connection())?["state"],
        "unconfigured"
    );
    assert_eq!(reopened.rules().len(), 2);
    Ok(())
}

#[tokio::test]
async fn a_desktop_delivery_pending_at_shutdown_survives_an_offline_stream() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db");
    let rule = rule("retry");
    let stream: Stream = serde_json::from_value(stream_json())?;
    {
        let mut db = Store::open(&path)?;
        db.observe(&rule, &[stream], true, Utc::now())?;
        assert!(db.recent_history()?.is_empty());
    }
    let server = twitch_server().await;
    Mock::given(wiremock::matchers::path("/helix/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .mount(&server)
        .await;
    let fake = FakeNotifier::default();
    let mut twitch = Twitch::for_test(http::client()?, &server.uri());
    let mut db = Store::open(&path)?;
    let mut poller = poll::Poller {
        store: &mut db,
        notifier: &fake,
        twitch: &mut twitch,
        notify_existing: true,
    };
    let (_sender, mut stop) = watch::channel(false);
    for _ in 0..2 {
        let report = poller.poll(std::slice::from_ref(&rule), &mut stop).await?;
        assert!(report.warning.is_none() && report.live.is_empty());
    }
    assert_eq!(db.recent_history()?.len(), 1);
    assert!(db.pending_desktop(&rule.id)?.is_empty());
    Ok(())
}

#[test]
fn a_failed_email_enqueue_rolls_back_first_scan_and_desktop_event() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("fixtures.db");
    let mut db = Store::open(&path)?;
    let fixture = Connection::open(&path)?;
    fixture.execute_batch("CREATE TRIGGER fail_fixture BEFORE INSERT ON mail_events BEGIN SELECT RAISE(ABORT,'fixture'); END;")?;
    let mut rule = rule("atomic");
    rule.channels.email = true;
    let stream: Stream = serde_json::from_value(stream_json())?;
    let failed = db.observe(&rule, std::slice::from_ref(&stream), true, Utc::now());
    assert!(failed.is_err());
    for table in ["metadata", "desktop_events", "mail_events", "mail_batches"] {
        assert_eq!(
            fixture.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))?,
            0
        );
    }
    fixture.execute_batch("DROP TRIGGER fail_fixture")?;
    db.observe(&rule, &[stream], true, Utc::now())?;
    assert_eq!(db.pending_desktop(&rule.id)?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn deleted_mail_is_reconciled_on_restart_and_paused_rules_keep_their_messages() -> Result<()>
{
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    async fn diagnostics(control: &Arc<Control>) -> Result<serde_json::Value> {
        let request = Request::builder()
            .uri("/api/mail/delivery")
            .header("host", "127.0.0.1:37219")
            .body(Body::empty())?;
        let response = web::router(control.clone(), 37219).oneshot(request).await?;
        Ok(serde_json::from_slice(
            &to_bytes(response.into_body(), 4096).await?,
        )?)
    }
    let dir = tempfile::tempdir()?;
    let db = dir.path().join("db");
    let config = dir.path().join("settings.json");
    let mut removed = rule("removed");
    removed.channels.email = true;
    let mut paused = rule("paused");
    paused.channels.email = true;
    let settings = Settings {
        rules: Some(vec![removed.clone(), paused.clone()]),
        ..Settings::defaults(None)
    };
    settings.save(&config)?;
    let store = Store::open_for_rules(&db, settings.rules.as_deref())?;
    let conn = Connection::open(&db)?;
    let seed = |rule: &Rule, id: &str| {
        let mut stream: Stream = serde_json::from_value(stream_json())?;
        stream.id = id.into();
        crate::mail::enqueue(
            &conn,
            &rule.id,
            &rule.name,
            &rule.filter.category,
            &stream,
            &rule.mail,
            Utc::now(),
        )
    };
    for id in ["queued", "sent", "claimed"] {
        seed(&removed, id)?;
    }
    seed(&paused, "waiting")?;
    conn.execute("UPDATE mail_batches SET state='sent',sent_at=unixepoch() WHERE id=(SELECT batch_id FROM mail_events WHERE stream_id='sent')",[])?;
    conn.execute("UPDATE mail_batches SET state='sending',attempts=1 WHERE id=(SELECT batch_id FROM mail_events WHERE stream_id='claimed')",[])?;
    conn.execute("UPDATE mail_batches SET last_error='temporary',failed_at=unixepoch(),next_attempt=unixepoch()+300 WHERE state='pending'",[])?;
    let (control, _) = Control::new(config.clone(), settings, None, Some(db.clone()));
    control.delete_rule("removed").await.unwrap();
    let active = diagnostics(&control).await?;
    assert_eq!(active["pending_count"], 1);
    assert_eq!(active["rule_name"], paused.name);
    assert!(active["next_attempt_at"].is_number());
    paused.enabled = false;
    control.upsert_rule(paused.clone()).await.unwrap();
    let inactive = diagnostics(&control).await?;
    assert_eq!(inactive["available"], true);
    assert_eq!(inactive["pending_count"], 0);
    assert_eq!(inactive["failed_count"], 0);
    assert!(inactive["next_attempt_at"].is_null() && inactive["last_error"].is_null());
    // A poll already holding the deleted rule cannot re-create a delivery.
    seed(&removed, "late")?;
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM mail_events WHERE stream_id IN ('late','queued')",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        0
    );
    drop(store);
    drop(control);
    // Simulate a previous release interrupted after saving its updated rule list.
    seed(&rule("formerly-deleted"), "orphan")?;
    let saved = Settings::load(&config, Settings::defaults(None));
    let restored = Store::open_for_rules(&db, saved.rules.as_deref())?;
    assert_eq!(restored.recent_history()?.len(), 1);
    assert_eq!(restored.recent_history()?[0].stream_id, "sent");
    assert_eq!(
        conn.query_row("SELECT count(*) FROM mail_events", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    let (reopened, _) = Control::new(config, saved, None, Some(db));
    assert!(diagnostics(&reopened).await?["next_attempt_at"].is_null());
    paused.enabled = true;
    reopened.upsert_rule(paused).await.unwrap();
    let resumed = diagnostics(&reopened).await?;
    assert_eq!(resumed["pending_count"], 1);
    assert_eq!(resumed["failed_count"], 1);
    assert!(resumed["next_attempt_at"].is_number());
    Ok(())
}
