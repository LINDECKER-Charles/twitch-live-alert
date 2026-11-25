use super::*;
use crate::store::Store;
use chrono::TimeZone;

fn query(days: u32, channel: &str, offset: u32) -> AnalyticsQuery {
    AnalyticsQuery {
        days,
        channel: channel.into(),
        offset,
    }
}

fn local_noon(day: u32) -> DateTime<Utc> {
    Local
        .with_ymd_and_hms(2026, 11, day, 12, 0, 0)
        .unwrap()
        .to_utc()
}

#[test]
fn combines_channels_by_identity_and_counts_deliveries_separately() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("analytics.db");
    let _store = Store::open(&path)?;
    let db = Connection::open(path)?;
    let start = local_noon(25).timestamp() - 7200;
    db.execute("INSERT INTO stream_sessions
        (stream_id,user_id,user_login,user_name,title,started_at,first_seen_at,last_seen_at,peak_viewers) VALUES
        ('live', '123', 'hunter', 'Hunter', 'Défis', ?1, ?1 + 600, ?1 + 3600, 25),
        ('legacy', NULL, 'HUNTER', 'Hunter', 'Archives', NULL, ?2, NULL, NULL),
        ('other', '20', 'dream', 'Dream', '', ?1, ?1, ?1, 0),
        ('before', '123', 'oldername', 'OlderName', '', NULL, ?3, NULL, NULL)",
        params![start, local_noon(19).timestamp(), local_noon(16).timestamp()])?;
    db.execute_batch("INSERT INTO desktop_events(rule_id,stream_id,rule_name,user_name,title,url,sent_at)
        VALUES ('one','live','One','Hunter','','',1), ('two','live','Two','Hunter','','',2),
               ('one','other','One','Dream','','',NULL);
        INSERT INTO mail_batches(id,rule_id,policy,next_attempt,state,sent_at)
        VALUES ('a','one','{}',0,'sent',3), ('b','two','{}',0,'pending',NULL);
        INSERT INTO mail_events(rule_id,stream_id,rule_name,user_name,title,url,batch_id,context)
        VALUES ('one','live','One','Hunter','','','a','{}'),('two','live','Two','Hunter','','','b','{}');")?;

    let now = local_noon(25);
    let selected = report(&db, &query(7, "id:123", 0), now)?;
    assert_eq!(selected.period_days, 7);
    assert_eq!(selected.channels.len(), 2);
    assert_eq!(selected.channels[0].name, "Hunter");
    assert_eq!(selected.channels[0].measurements.live_count, 2);
    assert_eq!(selected.totals.measurements.live_count, 2);
    assert_eq!(selected.totals.measurements.measured_count, 1);
    assert_eq!(selected.totals.measurements.total_seconds, 3600);
    assert_eq!(
        (selected.totals.desktop_alerts, selected.totals.email_alerts),
        (2, 1)
    );
    assert_eq!(selected.weekdays, [0, 0, 1, 1, 0, 0, 0]);
    assert_eq!(selected.sessions[0].peak_viewers, Some(25));
    assert_eq!(selected.sessions[1].duration_seconds, None);
    assert_eq!(selected.sessions[1].started_at, None);

    let all = report(&db, &query(0, "", 0), now)?;
    assert_eq!(all.period_days, 10);
    assert_eq!(all.totals.measurements.live_count, 4);
    assert_eq!(all.totals.measurements.measured_count, 2);
    assert_eq!(all.channels[0].measurements.live_count, 3);
    assert_eq!(all.channels[0].name, "Hunter");
    assert_eq!(all.sessions[1].duration_seconds, Some(0));
    Ok(())
}

#[test]
fn paginates_details_after_filtering_and_keeps_full_aggregates() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("page.db");
    let _store = Store::open(&path)?;
    let db = Connection::open(path)?;
    // Reused logins are ambiguous: a legacy session must stay unattributed to either ID.
    let now = local_noon(25);
    db.execute(
        "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i+1 FROM n WHERE i<57)
        INSERT INTO stream_sessions(stream_id,user_id,user_login,user_name,title,first_seen_at)
        SELECT printf('%02d',i), CASE WHEN i=57 THEN NULL WHEN i%2=0 THEN 'a' ELSE 'b' END,
               'shared','Shared','',?1-i FROM n",
        [now.timestamp()],
    )?;
    let page = report(&db, &query(30, "id:a", 25), now)?;
    assert_eq!(page.offset, 25);
    assert_eq!(page.totals.measurements.live_count, 29);
    assert_eq!(page.weekdays.iter().sum::<u64>(), 29);
    assert_eq!(page.sessions.len(), 4);
    assert_eq!(page.sessions[0].stream_id, "50");
    assert_eq!(page.channels.len(), 3);
    let unknown = report(&db, &query(30, "login:shared", 0), now)?;
    assert_eq!(unknown.sessions.len(), 1);
    assert_eq!(unknown.sessions[0].stream_id, "57");
    Ok(())
}
