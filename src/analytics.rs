use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Local, Utc};
use rusqlite::{Connection, OpenFlags, Row, params};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path, time::Duration};

#[cfg(test)]
mod tests;

const PAGE_SIZE: usize = 25;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnalyticsQuery {
    #[serde(default = "default_days")]
    days: u32,
    #[serde(default)]
    channel: String,
    #[serde(default)]
    offset: u32,
}

fn default_days() -> u32 {
    30
}

impl AnalyticsQuery {
    pub(crate) fn is_valid(&self) -> bool {
        matches!(self.days, 0 | 7 | 30 | 90) && self.channel.chars().count() <= 256
    }
}

#[derive(Default, Serialize)]
struct Measurements {
    live_count: u64,
    measured_count: u64,
    total_seconds: u64,
}

impl Measurements {
    fn add(&mut self, duration: Option<u64>) {
        self.live_count += 1;
        if let Some(seconds) = duration {
            self.measured_count += 1;
            self.total_seconds += seconds;
        }
    }
}

#[derive(Serialize)]
struct Channel {
    key: String,
    name: String,
    last_at: i64,
    #[serde(flatten)]
    measurements: Measurements,
}

#[derive(Default, Serialize)]
struct Totals {
    #[serde(flatten)]
    measurements: Measurements,
    desktop_alerts: i64,
    email_alerts: i64,
}

#[derive(Serialize)]
struct Session {
    stream_id: String,
    user_name: String,
    title: String,
    occurred_at: i64,
    started_at: Option<i64>,
    last_seen_at: Option<i64>,
    duration_seconds: Option<u64>,
    peak_viewers: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct Analytics {
    period_days: i64,
    channels: Vec<Channel>,
    totals: Totals,
    weekdays: [u64; 7],
    sessions: Vec<Session>,
    offset: u32,
}

pub(crate) fn read(path: &Path, query: &AnalyticsQuery) -> Result<Analytics> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(Duration::from_secs(3))?;
    report(&db, query, Utc::now())
}

// A historic login can be linked to a Twitch ID only if it has a single known owner.
// Do this lookup across all dates, so changing the period keeps the same identity.
const SESSIONS: &str = "
    SELECT s.stream_id, s.title, s.started_at, s.last_seen_at, s.peak_viewers,
        coalesce(s.started_at, s.first_seen_at) AS occurred_at,
        coalesce(nullif(s.user_name, ''), s.user_login, 'Chaîne inconnue') AS display_name,
        coalesce('id:' || coalesce(s.user_id, (
            SELECT min(owner.user_id) FROM stream_sessions owner
            WHERE owner.user_login = s.user_login COLLATE NOCASE AND owner.user_id IS NOT NULL
            HAVING min(owner.user_id) = max(owner.user_id)
        )), 'login:' || lower(s.user_login), 'name:' || nullif(lower(s.user_name), ''), 'unknown') AS channel_key,
        (SELECT count(*) FROM desktop_events WHERE stream_id = s.stream_id AND sent_at IS NOT NULL) AS desktop_alerts,
        (SELECT count(*) FROM mail_events e JOIN mail_batches b ON b.id = e.batch_id
         WHERE e.stream_id = s.stream_id AND b.state = 'sent') AS email_alerts
    FROM stream_sessions s
    WHERE coalesce(s.started_at, s.first_seen_at) BETWEEN
        CASE WHEN ?1 = 0 THEN 0
             ELSE unixepoch(?2, 'unixepoch', 'localtime', 'start of day', printf('%d days', 1 - ?1), 'utc')
             END AND ?2
    ORDER BY occurred_at DESC, s.stream_id";

impl Session {
    fn from_row(
        row: &Row<'_>,
        occurred_at: i64,
        duration_seconds: Option<u64>,
    ) -> rusqlite::Result<Self> {
        Ok(Self {
            stream_id: row.get("stream_id")?,
            user_name: row.get("display_name")?,
            title: row.get("title")?,
            occurred_at,
            started_at: row.get("started_at")?,
            last_seen_at: row.get("last_seen_at")?,
            duration_seconds,
            peak_viewers: row.get("peak_viewers")?,
        })
    }
}

fn report(db: &Connection, query: &AnalyticsQuery, now: DateTime<Utc>) -> Result<Analytics> {
    // One ordered SQLite snapshot serves both aggregates and detail. Only the requested
    // page and one summary per channel need to stay in memory.
    let mut stmt = db.prepare(SESSIONS)?;
    let mut rows = stmt.query(params![query.days, now.timestamp()])?;
    let mut channels = HashMap::new();
    let mut totals = Totals::default();
    let mut sessions = Vec::new();
    let mut weekdays = [0; 7];
    let today = now.with_timezone(&Local).date_naive();
    let mut earliest = today;
    while let Some(row) = rows.next()? {
        let occurred_at = row.get("occurred_at")?;
        let day = DateTime::from_timestamp(occurred_at, 0)
            .context("Date de direct illisible")?
            .with_timezone(&Local)
            .date_naive();
        earliest = day;
        let start: Option<i64> = row.get("started_at")?;
        let last: Option<i64> = row.get("last_seen_at")?;
        let duration = start.zip(last).and_then(|(a, b)| u64::try_from(b - a).ok());
        let key: String = row.get("channel_key")?;
        let selected = query.channel.is_empty() || key == query.channel;
        let entry = channels.entry(key.clone()).or_insert_with(|| Channel {
            key,
            name: String::new(),
            last_at: occurred_at,
            measurements: Measurements::default(),
        });
        if entry.name.is_empty() {
            entry.name = row.get("display_name")?;
        }
        entry.measurements.add(duration);
        if selected {
            if totals.measurements.live_count >= u64::from(query.offset)
                && sessions.len() < PAGE_SIZE
            {
                sessions.push(Session::from_row(row, occurred_at, duration)?);
            }
            totals.measurements.add(duration);
            totals.desktop_alerts += row.get::<_, i64>("desktop_alerts")?;
            totals.email_alerts += row.get::<_, i64>("email_alerts")?;
            weekdays[day.weekday().num_days_from_monday() as usize] += 1;
        }
    }
    let mut channels: Vec<_> = channels.into_values().collect();
    channels.sort_unstable_by(|a, b| {
        b.measurements
            .live_count
            .cmp(&a.measurements.live_count)
            .then_with(|| b.last_at.cmp(&a.last_at))
            .then_with(|| a.key.cmp(&b.key))
    });
    let period_days = if query.days == 0 {
        (today - earliest).num_days() + 1
    } else {
        query.days.into()
    };
    Ok(Analytics {
        period_days,
        channels,
        totals,
        weekdays,
        sessions,
        offset: query.offset,
    })
}
