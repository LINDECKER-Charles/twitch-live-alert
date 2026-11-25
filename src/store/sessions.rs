use crate::twitch::Stream;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, Statement, params};
use serde_json::Value;

// A snapshot-only backfill uses its Twitch start as its provisional first_seen_at.
// A later poll replaces that provisional timestamp with the first actual sighting.
const UPSERT: &str = "
    INSERT INTO stream_sessions(stream_id, user_id, user_login, user_name, title,
                                started_at, first_seen_at, last_seen_at, peak_viewers)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
    ON CONFLICT(stream_id) DO UPDATE SET
        user_id = coalesce(stream_sessions.user_id, excluded.user_id),
        user_login = coalesce(
            CASE WHEN excluded.last_seen_at >= coalesce(stream_sessions.last_seen_at, excluded.last_seen_at)
                 THEN excluded.user_login END,
            stream_sessions.user_login, excluded.user_login),
        user_name = coalesce(
            CASE WHEN excluded.last_seen_at >= coalesce(stream_sessions.last_seen_at, excluded.last_seen_at)
                 THEN nullif(excluded.user_name, '') END,
            nullif(stream_sessions.user_name, ''), excluded.user_name),
        title = coalesce(
            CASE WHEN excluded.last_seen_at >= coalesce(stream_sessions.last_seen_at, excluded.last_seen_at)
                 THEN nullif(excluded.title, '') END,
            nullif(stream_sessions.title, ''), excluded.title),
        started_at = coalesce(stream_sessions.started_at, excluded.started_at),
        first_seen_at = CASE WHEN stream_sessions.last_seen_at IS NULL
                                  AND stream_sessions.first_seen_at = stream_sessions.started_at
                                  AND excluded.last_seen_at IS NOT NULL
                             THEN excluded.first_seen_at
                             ELSE min(stream_sessions.first_seen_at, excluded.first_seen_at) END,
        last_seen_at = coalesce(max(stream_sessions.last_seen_at, excluded.last_seen_at),
                                stream_sessions.last_seen_at, excluded.last_seen_at),
        peak_viewers = coalesce(max(stream_sessions.peak_viewers, excluded.peak_viewers),
                                stream_sessions.peak_viewers, excluded.peak_viewers)";

fn timestamp(raw: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|value| value.timestamp())
}

fn started_before(raw: &str, seen_at: i64) -> Option<i64> {
    timestamp(raw).filter(|value| (1..=seen_at).contains(value))
}

fn login(raw: &str) -> Option<String> {
    let raw = raw.trim();
    (!raw.is_empty()
        && raw
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || value == b'_'))
    .then(|| raw.to_ascii_lowercase())
}

fn login_from_url(url: &str) -> Option<String> {
    login(
        url.strip_prefix("https://www.twitch.tv/")
            .or_else(|| url.strip_prefix("https://twitch.tv/"))?,
    )
}

struct Session<'a> {
    stream_id: &'a str,
    user_id: &'a str,
    user_login: Option<String>,
    user_name: &'a str,
    title: &'a str,
    started_at: Option<i64>,
    first_seen_at: i64,
    last_seen_at: Option<i64>,
    peak_viewers: Option<i64>,
}

impl<'a> Session<'a> {
    fn from_stream(stream: &'a Stream, first_seen_at: i64, last_seen_at: Option<i64>) -> Self {
        Self {
            stream_id: &stream.id,
            user_id: &stream.user_id,
            user_login: login(&stream.user_login),
            user_name: &stream.user_name,
            title: &stream.title,
            started_at: started_before(&stream.started_at, last_seen_at.unwrap_or(first_seen_at)),
            first_seen_at,
            last_seen_at,
            peak_viewers: stream.viewer_count.try_into().ok(),
        }
    }

    fn from_desktop(row: &'a Row<'_>, first_seen_at: i64) -> Result<Self> {
        Ok(Self {
            stream_id: row.get_ref(0)?.as_str()?,
            user_id: "",
            user_login: login_from_url(row.get_ref(3)?.as_str()?),
            user_name: row.get_ref(1)?.as_str()?,
            title: row.get_ref(2)?.as_str()?,
            started_at: None,
            first_seen_at,
            last_seen_at: None,
            peak_viewers: None,
        })
    }

    fn save(self, upsert: &mut Statement<'_>) -> Result<()> {
        let user_id = self.user_id.trim();
        let user_name = match self.user_name.trim() {
            "Chaîne inconnue" => "",
            value => value,
        };
        upsert.execute(params![
            self.stream_id,
            (!user_id.is_empty()).then_some(user_id),
            self.user_login,
            user_name,
            self.title,
            self.started_at,
            self.first_seen_at,
            self.last_seen_at,
            self.peak_viewers,
        ])?;
        Ok(())
    }
}

pub(super) fn observe(
    db: &Connection,
    streams: &[Stream],
    observed_at: DateTime<Utc>,
) -> Result<()> {
    let timestamp = observed_at.timestamp();
    let mut upsert = db.prepare_cached(UPSERT)?;
    for stream in streams {
        Session::from_stream(stream, timestamp, Some(timestamp)).save(&mut upsert)?;
    }
    Ok(())
}

pub(super) fn restore_sent_stream(db: &Connection, stream: &Stream, sent_at: i64) -> Result<()> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM stream_sessions WHERE stream_id=?1)",
        [&stream.id],
        |row| row.get(0),
    )?;
    if !exists {
        let mut upsert = db.prepare_cached(UPSERT)?;
        Session::from_stream(stream, sent_at, None).save(&mut upsert)?;
    }
    Ok(())
}

pub(super) fn restore_pending_desktop(
    db: &Connection,
    rule_id: &str,
    stream_id: &str,
    sent_at: i64,
) -> Result<()> {
    let mut stmt = db.prepare_cached(
        "SELECT stream_id, user_name, title, url, snapshot FROM desktop_events e
         WHERE rule_id=?1 AND stream_id=?2 AND sent_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM stream_sessions s WHERE s.stream_id=e.stream_id)",
    )?;
    let mut rows = stmt.query(params![rule_id, stream_id])?;
    if let Some(row) = rows.next()? {
        let mut upsert = db.prepare_cached(UPSERT)?;
        if let Some(stream) = read_snapshot(row, 4)? {
            Session::from_stream(&stream, sent_at, None).save(&mut upsert)?;
        } else {
            Session::from_desktop(row, sent_at)?.save(&mut upsert)?;
        }
    }
    Ok(())
}

pub(super) fn migrate(db: &Connection, has_legacy_baselines: bool) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE stream_sessions (
            stream_id TEXT NOT NULL PRIMARY KEY,
            user_id TEXT,
            user_login TEXT,
            user_name TEXT NOT NULL,
            title TEXT NOT NULL,
            started_at INTEGER,
            first_seen_at INTEGER NOT NULL,
            last_seen_at INTEGER,
            peak_viewers INTEGER
        );
        CREATE INDEX stream_sessions_dates ON stream_sessions(coalesce(started_at, first_seen_at) DESC, stream_id);
        CREATE INDEX stream_sessions_login_identity ON stream_sessions(user_login COLLATE NOCASE, user_id) WHERE user_id IS NOT NULL;
        CREATE INDEX desktop_events_report_stream ON desktop_events(stream_id) WHERE sent_at IS NOT NULL;
        CREATE INDEX mail_events_report_stream ON mail_events(stream_id, batch_id);",
    )?;
    if has_legacy_baselines {
        // Up to v2 the baseline was timestamped when an already live stream was seen.
        db.execute_batch(
            "INSERT INTO stream_sessions(stream_id, user_name, title, first_seen_at, last_seen_at)
             SELECT stream_id, '', '', recorded_at, recorded_at FROM deliveries WHERE disposition='baseline';
             DELETE FROM deliveries WHERE disposition='baseline';",
        )?;
    }
    let mut upsert = db.prepare(UPSERT)?;
    backfill_desktop(db, &mut upsert)?;
    backfill_mail(db, &mut upsert)?;
    backfill_pending_desktop(db, &mut upsert)
}

fn backfill_desktop(db: &Connection, upsert: &mut Statement<'_>) -> Result<()> {
    let mut stmt = db.prepare(
        "SELECT stream_id, user_name, title, url, sent_at FROM desktop_events
         WHERE sent_at IS NOT NULL ORDER BY sent_at, rowid",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        Session::from_desktop(row, row.get(4)?)?.save(upsert)?;
    }
    Ok(())
}

fn backfill_mail(db: &Connection, upsert: &mut Statement<'_>) -> Result<()> {
    let mut stmt = db.prepare(
        "SELECT e.stream_id, e.user_name, e.title, e.url, e.context,
                CASE WHEN b.state='sent' THEN b.sent_at END
         FROM mail_events e JOIN mail_batches b ON b.id=e.batch_id ORDER BY e.rowid",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let context: Value = serde_json::from_str(row.get_ref(4)?.as_str()?).unwrap_or_default();
        let field = |key: &str| context[key].as_str().unwrap_or_default();
        let detected_at = timestamp(field("detected_at"));
        let sent_at: Option<i64> = row.get(5)?;
        let Some(first_seen_at) = detected_at.or(sent_at) else {
            continue;
        };
        Session {
            stream_id: row.get_ref(0)?.as_str()?,
            user_id: field("user_id"),
            user_login: login(field("user_login")).or(login_from_url(row.get_ref(3)?.as_str()?)),
            user_name: row.get_ref(1)?.as_str()?,
            title: row.get_ref(2)?.as_str()?,
            started_at: started_before(field("started_at"), first_seen_at),
            first_seen_at,
            last_seen_at: detected_at,
            peak_viewers: field("viewer_count")
                .parse()
                .ok()
                .filter(|count| *count >= 0),
        }
        .save(upsert)?;
    }
    Ok(())
}

fn backfill_pending_desktop(db: &Connection, upsert: &mut Statement<'_>) -> Result<()> {
    // Before v5 this snapshot had no detection timestamp. If it is the sole
    // source, its Twitch start locates the session until there is an observation.
    let now = Utc::now().timestamp();
    let mut known = db.prepare("SELECT first_seen_at FROM stream_sessions WHERE stream_id=?")?;
    let mut stmt = db.prepare(
        "SELECT stream_id, snapshot FROM desktop_events WHERE sent_at IS NULL AND snapshot IS NOT NULL",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        if let Some(stream) = read_snapshot(row, 1)? {
            let first_seen_at: Option<i64> =
                known.query_row([&stream.id], |row| row.get(0)).optional()?;
            if let Some(first_seen_at) =
                first_seen_at.or_else(|| started_before(&stream.started_at, now))
            {
                Session::from_stream(&stream, first_seen_at, None).save(upsert)?;
            }
        }
    }
    Ok(())
}

fn read_snapshot(row: &Row<'_>, column: usize) -> Result<Option<Stream>> {
    let id = row.get_ref(0)?.as_str()?;
    let snapshot: Option<&str> = row.get_ref(column)?.as_str_or_null()?;
    Ok(snapshot
        .and_then(|json| serde_json::from_str::<Stream>(json).ok())
        .filter(|stream| stream.id == id))
}

#[cfg(test)]
mod tests;
