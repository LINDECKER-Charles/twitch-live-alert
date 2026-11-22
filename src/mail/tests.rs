use super::{
    template::{self, Context},
    *,
};
use crate::{schedule::Window, twitch::Stream};
use anyhow::Result;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

mod smtp;

fn utc(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn stream(id: &str) -> Stream {
    serde_json::from_value(json!({
        "id":id,"user_login":"hunter","user_name":"Hunter","game_id":"460636","language":"fr","type":"live",
        "title":format!("Live {id}"),"started_at":"2026-05-01T08:00:00Z","viewer_count":5
    })).unwrap()
}

fn digest(zone: &str, clock: &str) -> MailPolicy {
    MailPolicy {
        mode: MailMode::Digest,
        schedule: Schedule {
            timezone: zone.to_owned(),
            windows: vec![],
        },
        digest_times: vec![clock.into()],
    }
}

fn configuration(port: u16, security: &str) -> Value {
    json!({
        "host":"127.0.0.1","port":port,"security":security,"sender":"Service <alerts@example.invalid>",
        "recipients":["hunter@example.invalid"],"subject_template":"{{user_name}} · {{rule}}",
        "body_template":"{{title}}\n{{url}}"
    })
}

async fn request(app: &Router, method: &str, uri: &str, object: Value) -> (StatusCode, Value) {
    let body = if method == "GET" {
        Body::empty()
    } else {
        Body::from(object.to_string())
    };
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let json =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    (status, json)
}

#[test]
fn windows_include_overnight_previous_day_and_exclude_end() {
    let schedule = Schedule {
        timezone: "Europe/Paris".into(),
        windows: vec![Window {
            days: vec![1],
            start: "22:00".into(),
            end: "02:00".into(),
        }],
    };
    schedule.validate().unwrap();
    assert!(schedule.allows(utc("2026-01-05T21:00:00Z"))); // Monday 22:00
    assert!(schedule.allows(utc("2026-01-06T00:59:59Z"))); // Tuesday 01:59
    assert!(!schedule.allows(utc("2026-01-06T01:00:00Z")));
    assert_eq!(
        schedule.next_window(utc("2026-01-06T01:00:00Z")).unwrap(),
        utc("2026-01-12T21:00:00Z")
    );
    let all_day = Schedule {
        timezone: "UTC".into(),
        windows: vec![Window {
            days: vec![7],
            start: "12:00".into(),
            end: "12:00".into(),
        }],
    };
    assert!(all_day.allows(utc("2026-01-04T00:00:00Z")));
    assert!(!all_day.allows(utc("2026-01-05T00:00:00Z")));
    assert!(Schedule::default().allows(utc("2026-01-04T00:00:00Z")));
}

#[test]
fn paris_dst_gap_moves_to_first_valid_minute_and_fold_only_runs_once() {
    let p = digest("Europe/Paris", "02:30");
    let next = |stamp| p.schedule.next_digest(&p.digest_times, utc(stamp)).unwrap();
    assert_eq!(next("2026-03-28T23:00:00Z"), utc("2026-03-29T01:00:00Z"));
    assert_eq!(next("2026-10-24T23:00:00Z"), utc("2026-10-25T00:30:00Z"));
    assert_eq!(next("2026-10-25T00:31:00Z"), utc("2026-10-26T01:30:00Z"));
    let mut p = p;
    p.schedule.windows = vec![Window {
        days: vec![7],
        start: "10:00".into(),
        end: "11:00".into(),
    }];
    assert!(p.validate().is_err());
    p.digest_times = vec!["10:00".into(), "10:30".into()];
    assert!(p.validate().is_ok());
    assert_eq!(
        p.schedule
            .next_digest(&p.digest_times, utc("2026-10-25T09:01:00Z"))
            .unwrap(),
        utc("2026-10-25T09:30:00Z")
    );
    p.schedule.timezone = "invented-zone".into();
    assert!(p.validate().is_err());
}

#[test]
fn template_handles_missing_fields_escaping_and_untrusted_twitch_title() {
    let mut stream = stream("100");
    stream.title = "{{rule}}\r\nBcc: hidden".into();
    let ctx = Context::new("Test", "Bloodborne", &stream, utc("2026-01-01T00:00:00Z"));
    let text = template::render(
        "{{title}}",
        r"{{category}} / {{tags}} / {{is_mature}} / \{{rule}}",
        &ctx,
    )
    .unwrap();
    assert_eq!(text.subject, "{{rule}}  Bcc: hidden");
    assert_eq!(text.body, "Bloodborne / — / — / {{rule}}");
    assert!(template::render("Objet\r\nBcc: x", "corps", &ctx).is_err());
    assert!(template::render("{{absent}}", "corps", &ctx).is_err());
    assert!(template::render("{{broken", "corps", &ctx).is_err());
    for var in template::VARIABLES {
        assert!(template::render("Test", &format!("{{{{{var}}}}}"), &Context::sample()).is_ok());
    }
}

#[test]
fn outbox_migration_is_repeatable_and_detection_and_enqueue_roll_back_together() -> Result<()> {
    let mut db = Connection::open_in_memory()?;
    db.execute_batch("PRAGMA user_version=3; CREATE TABLE deliveries(stream_id TEXT PRIMARY KEY); INSERT INTO deliveries VALUES('legacy');")?;
    migrate(&db)?;
    migrate(&db)?;
    let tx = db.transaction()?;
    let instant = MailPolicy::default();
    let now = utc("2026-01-01T08:00:00Z");
    enqueue(&tx, "a", "A", "Bloodborne", &stream("1"), &instant, now)?;
    tx.rollback()?;
    assert_eq!(
        db.query_row("SELECT count(*) FROM mail_batches", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    for rule in ["a", "a", "b"] {
        enqueue(&db, rule, "A", "Bloodborne", &stream("1"), &instant, now)?;
    }
    assert_eq!(
        db.query_row("SELECT count(*) FROM mail_events", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM mail_batches", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM deliveries", [], |r| r
            .get::<_, i64>(0))?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn draft_preview_uses_no_configuration_and_rejects_unsupported_tls() {
    let dir = TempDir::new().unwrap();
    let service = MailService::new(dir.path().into());
    let app = service.router();
    let (status, default) = request(&app, "GET", "/api/mail", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(default["security"], "tls");
    assert_eq!(default["port"], 465);
    assert_eq!(default["credentials_configured"], false);
    let (status, preview) = request(
        &app,
        "POST",
        "/api/mail/preview",
        json!({"subject_template":"{{user_login}}","body_template":"{{rule}}"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["subject"], "chasseuse_exemple");
    assert!(!service.file().exists());
    let mut config = configuration(465, "plain");
    config["password"] = Value::String("sentinel-test-value".into());
    let (status, error) = request(&app, "POST", "/api/mail", config).await;
    assert!(status.is_client_error());
    assert!(!error.to_string().contains("sentinel-test-value"));
}

#[cfg(windows)]
#[tokio::test]
async fn dpapi_settings_never_expose_auth_and_bind_it_to_server() -> Result<()> {
    let dir = TempDir::new()?;
    let service = MailService::new(dir.path().into());
    let app = service.router();
    let mut config = configuration(465, "tls");
    let mut first = config.clone();
    first["username"] = "synthetic-auth-user".into();
    first["password"] = "synthetic-auth-pass".into();
    let (status, saved) = request(&app, "POST", "/api/mail", first).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["credentials_configured"], true);
    let (_, readback) = request(&app, "GET", "/api/mail", Value::Null).await;
    let disk = std::fs::read_to_string(service.file())?;
    for text in [saved.to_string(), readback.to_string(), disk] {
        assert!(!text.contains("synthetic-auth"));
        assert!(!text.contains("\"password\""));
        assert!(!text.contains("\"username\""));
    }
    assert_eq!(
        settings::Stored::load(&service.file())?
            .auth()?
            .unwrap()
            .username,
        "synthetic-auth-user"
    );
    config["port"] = 587.into();
    assert_eq!(
        request(&app, "POST", "/api/mail", config.clone()).await.0,
        StatusCode::BAD_REQUEST
    );
    config["username"] = "".into();
    config["password"] = "".into();
    assert_eq!(
        request(&app, "POST", "/api/mail", config).await.1["credentials_configured"],
        false
    );
    Ok(())
}

#[test]
fn folded_hour_can_reopen_a_partially_overlapping_window() {
    let window = |start: &str, end: &str| Schedule {
        timezone: "Europe/Paris".into(),
        windows: vec![Window {
            days: vec![7],
            start: start.into(),
            end: end.into(),
        }],
    };
    let folded = window("01:15", "02:20");
    let closed = utc("2026-10-25T00:40:00Z");
    assert!(!folded.allows(closed));
    assert_eq!(
        folded.next_window(closed).unwrap(),
        utc("2026-10-25T01:00:00Z")
    );
    let skipped = window("02:15", "02:45");
    assert_eq!(
        skipped.next_window(utc("2026-03-29T00:40:00Z")).unwrap(),
        utc("2026-04-05T00:15:00Z")
    );
    let partial = window("02:15", "03:45");
    assert_eq!(
        partial.next_window(utc("2026-03-29T00:40:00Z")).unwrap(),
        utc("2026-03-29T01:00:00Z")
    );
}
#[tokio::test]
async fn unavailable_database_and_unknown_internal_diagnostics_are_safe() -> Result<()> {
    let dir = TempDir::new()?;
    let path = dir.path().join("absent.sqlite3");
    let app = MailService::new(dir.path().into())
        .with_database(Some(path.clone()))
        .router();
    let (_, unavailable) = request(&app, "GET", "/api/mail/delivery", Value::Null).await;
    assert_eq!(unavailable["available"], false);
    assert!(!path.exists());
    let db = Connection::open(&path)?;
    migrate(&db)?;
    enqueue(
        &db,
        "a",
        "A",
        "Bloodborne",
        &stream("1"),
        &MailPolicy::default(),
        Utc::now(),
    )?;
    db.execute("UPDATE mail_batches SET last_error='synthetic private text from server',failed_at=unixepoch()",[])?;
    let (_, state) = request(&app, "GET", "/api/mail/delivery", Value::Null).await;
    assert_eq!(state["available"], true);
    assert_eq!(state["error_code"], "configuration");
    assert!(!state.to_string().contains("synthetic"));
    Ok(())
}
