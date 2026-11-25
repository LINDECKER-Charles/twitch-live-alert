use super::*;
use crate::{rules::Rule, store::Store, twitch::StreamFilter};

const STARTED_AT: i64 = 1_767_895_200;

fn stream(id: &str) -> Stream {
    Stream {
        id: id.into(),
        user_id: "123".into(),
        user_login: "Hunter_1".into(),
        user_name: "Chasseuse".into(),
        title: "Yharnam".into(),
        game_name: "Bloodborne".into(),
        game_id: "460636".into(),
        language: "fr".into(),
        started_at: time(STARTED_AT).to_rfc3339(),
        viewer_count: 18,
        tags: vec![],
        thumbnail_url: String::new(),
        is_mature: None,
        kind: "live".into(),
    }
}

fn time(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).unwrap()
}

#[derive(Debug, PartialEq, Eq)]
struct StoredSession {
    user_id: Option<String>,
    login: Option<String>,
    name: String,
    title: String,
    started: Option<i64>,
    first_seen: i64,
    last_seen: Option<i64>,
    peak: Option<i64>,
}

fn stored(db: &Connection, id: &str) -> Result<StoredSession> {
    Ok(db.query_row(
        "SELECT user_id, user_login, user_name, title, started_at,
                first_seen_at, last_seen_at, peak_viewers FROM stream_sessions WHERE stream_id=?",
        [id],
        |row| {
            Ok(StoredSession {
                user_id: row.get(0)?,
                login: row.get(1)?,
                name: row.get(2)?,
                title: row.get(3)?,
                started: row.get(4)?,
                first_seen: row.get(5)?,
                last_seen: row.get(6)?,
                peak: row.get(7)?,
            })
        },
    )?)
}

#[test]
fn tracks_baselines_and_repeated_polls_across_rules_as_one_session() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("alerts.sqlite3"))?;
    let mut first_rule = Rule::legacy(StreamFilter::default());
    first_rule.channels.email = true;
    let mut second_rule = first_rule.clone();
    second_rule.id = "another-rule".into();
    let mut stream = stream("live");

    store.observe(
        &first_rule,
        std::slice::from_ref(&stream),
        false,
        time(STARTED_AT + 120),
    )?;
    assert!(store.pending_desktop(&first_rule.id)?.is_empty());
    store.observe(
        &second_rule,
        std::slice::from_ref(&stream),
        true,
        time(STARTED_AT + 120),
    )?;
    assert_eq!(store.pending_desktop(&second_rule.id)?.len(), 1);

    stream.title = "Affrontement de boss".into();
    stream.viewer_count = 29;
    store.observe(
        &first_rule,
        std::slice::from_ref(&stream),
        false,
        time(STARTED_AT + 180),
    )?;
    stream.title = "Retour au rêve".into();
    stream.viewer_count = 4;
    store.observe(
        &second_rule,
        std::slice::from_ref(&stream),
        true,
        time(STARTED_AT + 240),
    )?;
    stream.title = "Observation plus ancienne".into();
    store.observe(&first_rule, &[stream], false, time(STARTED_AT + 150))?;
    store.observe(&first_rule, &[], false, time(STARTED_AT + 300))?;
    store.confirm_desktop(&second_rule.id, "live")?;

    let db = &store.connection;
    let sessions: i64 =
        db.query_row("SELECT count(*) FROM stream_sessions", [], |row| row.get(0))?;
    assert_eq!(sessions, 1);
    assert_eq!(
        stored(db, "live")?,
        StoredSession {
            user_id: Some("123".into()),
            login: Some("hunter_1".into()),
            name: "Chasseuse".into(),
            title: "Retour au rêve".into(),
            started: Some(STARTED_AT),
            first_seen: STARTED_AT + 120,
            last_seen: Some(STARTED_AT + 240),
            peak: Some(29),
        }
    );
    Ok(())
}

#[test]
fn migrates_desktop_alerts_and_uses_mail_detection_instead_of_send_time() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("alerts.sqlite3");
    let store = Store::open(&path)?;
    let db = &store.connection;
    db.execute_batch(
        "DROP TABLE stream_sessions;
         DROP INDEX desktop_events_report_stream;
         DROP INDEX mail_events_report_stream;
         PRAGMA user_version=4;
         INSERT INTO baselines VALUES ('initialized:fr:bloodborne', 'untimed');",
    )?;
    let desktop = |id, title, url, sent_at| {
        db.execute(
            "INSERT INTO desktop_events(rule_id,stream_id,rule_name,user_name,title,url,sent_at)
         VALUES ('a',?1,'A','Chaîne inconnue',?2,?3,?4)",
            params![id, title, url, sent_at],
        )
    };
    desktop(
        "shared",
        "Ancien titre",
        "https://www.twitch.tv/Hunter_1",
        STARTED_AT + 65,
    )?;
    desktop(
        "desktop",
        "Yharnam",
        "https://www.twitch.tv/Hunter_1",
        STARTED_AT + 90,
    )?;
    desktop(
        "unknown",
        "",
        "https://example.org/definitely_not_twitch",
        STARTED_AT + 95,
    )?;

    let mut rule = Rule::legacy(StreamFilter::default());
    rule.channels.email = true;
    for (id, observed_at) in [
        ("shared", STARTED_AT + 120),
        ("mail-pending", STARTED_AT + 130),
    ] {
        crate::mail::enqueue(
            db,
            &rule.id,
            &rule.name,
            &rule.filter.category,
            &stream(id),
            &rule.mail,
            time(observed_at),
        )?;
    }
    db.execute(
        "UPDATE mail_batches SET state='sent', sent_at=?1
         WHERE id=(SELECT batch_id FROM mail_events WHERE stream_id='shared')",
        [STARTED_AT + 3600],
    )?;
    db.execute(
        "INSERT INTO desktop_events(rule_id,stream_id,rule_name,user_name,title,url,snapshot)
         VALUES ('b','shared','B','Chasseuse','Yharnam','https://www.twitch.tv/hunter_1',?1)",
        [serde_json::to_string(&Stream {
            viewer_count: 27,
            ..stream("shared")
        })?],
    )?;
    for snapshot in [
        stream("desktop-pending"),
        Stream {
            started_at: "not a date".into(),
            ..stream("desktop-undated")
        },
    ] {
        db.execute(
            "INSERT INTO desktop_events(rule_id,stream_id,rule_name,user_name,title,url,snapshot)
             VALUES ('b',?1,'B',?2,?3,?4,?5)",
            params![
                snapshot.id,
                snapshot.user_name,
                snapshot.title,
                snapshot.url(),
                serde_json::to_string(&snapshot)?
            ],
        )?;
    }
    drop(store);

    for _ in 0..2 {
        let store = Store::open(&path)?;
        let db = &store.connection;
        assert_eq!(
            db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?,
            5
        );
        assert_eq!(
            stored(db, "shared")?,
            StoredSession {
                user_id: Some("123".into()),
                login: Some("hunter_1".into()),
                name: "Chasseuse".into(),
                title: "Yharnam".into(),
                started: Some(STARTED_AT),
                first_seen: STARTED_AT + 65,
                last_seen: Some(STARTED_AT + 120),
                peak: Some(27),
            }
        );
        assert_eq!(
            stored(db, "desktop")?,
            StoredSession {
                user_id: None,
                login: Some("hunter_1".into()),
                name: String::new(),
                title: "Yharnam".into(),
                started: None,
                first_seen: STARTED_AT + 90,
                last_seen: None,
                peak: None,
            }
        );
        assert_eq!(stored(db, "unknown")?.login, None);
        assert_eq!(
            stored(db, "mail-pending")?.last_seen,
            Some(STARTED_AT + 130)
        );
        assert!(store.contains("untimed", &StreamFilter::default())?);
        assert!(stored(db, "untimed").is_err());
        let pending = stored(db, "desktop-pending")?;
        assert_eq!(
            (
                pending.started,
                pending.first_seen,
                pending.last_seen,
                pending.peak
            ),
            (Some(STARTED_AT), STARTED_AT, None, Some(18))
        );
        assert!(stored(db, "desktop-undated").is_err());
        assert_eq!(store.recent_history()?.len(), 4);
    }
    let mut store = Store::open(&path)?;
    store.confirm_desktop("b", "desktop-undated")?;
    let sent_at = store.connection.query_row(
        "SELECT sent_at FROM desktop_events WHERE rule_id='b' AND stream_id='desktop-undated'",
        [],
        |row| row.get(0),
    )?;
    let confirmed = stored(&store.connection, "desktop-undated")?;
    assert_eq!(
        (confirmed.started, confirmed.first_seen, confirmed.last_seen),
        (None, sent_at, None)
    );

    store.observe(
        &rule,
        &[stream("desktop-pending")],
        true,
        time(STARTED_AT + 250),
    )?;
    let observed = stored(&store.connection, "desktop-pending")?;
    assert_eq!(
        (observed.first_seen, observed.last_seen),
        (STARTED_AT + 250, Some(STARTED_AT + 250))
    );
    Ok(())
}

#[test]
fn future_start_does_not_hide_a_valid_observation() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut store = Store::open(&directory.path().join("alerts.sqlite3"))?;
    let rule = Rule::legacy(StreamFilter::default());
    let mut future = stream("future");
    future.started_at = time(STARTED_AT + 600).to_rfc3339();
    store.observe(&rule, &[future], true, time(STARTED_AT))?;

    let observed = stored(&store.connection, "future")?;
    assert_eq!(
        (observed.started, observed.first_seen, observed.last_seen),
        (None, STARTED_AT, Some(STARTED_AT))
    );
    Ok(())
}
