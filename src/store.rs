use crate::rules::{LEGACY_ID, LEGACY_NAME, Rule};
use crate::twitch::{Stream, StreamFilter};
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, TransactionBehavior, functions::FunctionFlags, params,
};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    time::Duration,
};

mod sessions;

pub struct Store {
    connection: Connection,
    _lock: File,
}

#[derive(Clone, Serialize)]
pub struct Delivery {
    pub rule_id: String,
    pub rule_name: String,
    pub channel: String,
    pub stream_id: String,
    pub user_name: String,
    pub title: String,
    pub url: String,
    pub recorded_at: u64,
}

#[derive(Serialize)]
pub(crate) struct History {
    pub(crate) items: Vec<Delivery>,
    pub(crate) total: u64,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).context("Impossible de créer le répertoire de données")?;
        }
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .context("Impossible d'ouvrir le verrou de la base")?;
        lock.try_lock_exclusive()
            .context("Une autre instance utilise déjà cette base de données")?;
        let mut connection =
            Connection::open(path).context("Impossible d'ouvrir la base SQLite")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 5 {
            bail!("Version de base SQLite plus récente que ce service");
        }
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if version < 2 {
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS deliveries (
                     stream_id TEXT PRIMARY KEY,
                     disposition TEXT NOT NULL CHECK(disposition IN ('sent', 'baseline')),
                     message_id TEXT,
                     recorded_at INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 ALTER TABLE deliveries ADD COLUMN user_name TEXT;
                 ALTER TABLE deliveries ADD COLUMN title TEXT;
                 ALTER TABLE deliveries ADD COLUMN url TEXT;",
            )
            .context("Impossible de migrer la base SQLite")?;
        }
        if version < 3 {
            tx.execute_batch(
                "UPDATE metadata SET key='initialized:fr:bloodborne' WHERE key='initialized';
                 CREATE TABLE baselines (
                     filter_key TEXT NOT NULL,
                     stream_id TEXT NOT NULL,
                     PRIMARY KEY(filter_key, stream_id)
                 );
                 INSERT INTO baselines(filter_key, stream_id)
                     SELECT 'initialized:fr:bloodborne', stream_id FROM deliveries WHERE disposition='baseline';
                 CREATE INDEX delivery_dates ON deliveries(recorded_at DESC) WHERE disposition='sent';"
            ).context("Impossible de migrer les filtres de surveillance")?;
        }
        crate::mail::migrate(&tx)?;
        if version < 4 {
            tx.execute_batch(
                "CREATE TABLE desktop_events (
                    rule_id TEXT NOT NULL, stream_id TEXT NOT NULL, rule_name TEXT NOT NULL,
                    user_name TEXT NOT NULL, title TEXT NOT NULL, url TEXT NOT NULL,
                    snapshot TEXT, sent_at INTEGER,
                    PRIMARY KEY(rule_id, stream_id));
                 CREATE INDEX desktop_history ON desktop_events(sent_at DESC);
                 CREATE INDEX desktop_pending ON desktop_events(rule_id) WHERE sent_at IS NULL;",
            )?;
            tx.execute(
                "INSERT INTO desktop_events(rule_id,stream_id,rule_name,user_name,title,url,sent_at)
                 SELECT ?1,stream_id,?2,coalesce(user_name,'Chaîne inconnue'),coalesce(title,''),coalesce(url,''),recorded_at
                 FROM deliveries WHERE disposition='sent' ORDER BY rowid", params![LEGACY_ID, LEGACY_NAME])?;
            tx.execute_batch("CREATE VIEW alert_history AS
                SELECT rule_id, rule_name, 'desktop' channel, stream_id,user_name,title,url,sent_at recorded_at,rowid tie
                FROM desktop_events WHERE sent_at IS NOT NULL
                UNION ALL SELECT e.rule_id,e.rule_name,'email',e.stream_id,e.user_name,e.title,e.url,b.sent_at,e.rowid
                FROM mail_events e JOIN mail_batches b ON e.batch_id=b.id WHERE b.state='sent';")?;
        }
        if version < 5 {
            sessions::migrate(&tx, version < 3)
                .context("Impossible de migrer les observations Twitch")?;
            tx.pragma_update(None, "user_version", 5)?;
        }
        tx.commit()?;
        Ok(Self {
            connection,
            _lock: lock,
        })
    }

    pub(crate) fn open_for_rules(path: &Path, known: Option<&[Rule]>) -> Result<Self> {
        let mut store = Self::open(path)?;
        crate::mail::reconcile_rules(
            &mut store.connection,
            known.map(|rules| rules.iter().map(|r| r.id.as_str()).collect()),
        )?;
        Ok(store)
    }

    pub fn initialize(
        &mut self,
        filter: &StreamFilter,
        streams: &[Stream],
        notify_existing: bool,
    ) -> Result<()> {
        let tx = self.connection.transaction()?;
        sessions::observe(&tx, streams, chrono::Utc::now())?;
        Self::initialize_baseline(&tx, &filter.baseline_key(), streams, notify_existing)?;
        tx.commit()?;
        Ok(())
    }

    fn initialize_baseline(
        transaction: &rusqlite::Transaction<'_>,
        key: &str,
        streams: &[Stream],
        notify_existing: bool,
    ) -> Result<()> {
        if transaction.execute(
            "INSERT OR IGNORE INTO metadata(key,value) VALUES (?1,'true')",
            [key],
        )? > 0
            && !notify_existing
        {
            let mut insert = transaction
                .prepare("INSERT OR IGNORE INTO baselines(filter_key,stream_id) VALUES (?1,?2)")?;
            for stream in streams {
                insert.execute(params![key, stream.id])?;
            }
        }
        Ok(())
    }

    pub fn contains(&self, stream_id: &str, filter: &StreamFilter) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM desktop_events WHERE rule_id=?1 AND stream_id=?2 AND sent_at IS NOT NULL
             UNION ALL SELECT 1 FROM baselines WHERE filter_key=?3 AND stream_id=?2)",
            params![LEGACY_ID, stream_id, filter.baseline_key()], |r| r.get(0))?)
    }

    pub fn mark_sent(&self, stream: &Stream) -> Result<()> {
        let tx = self.connection.unchecked_transaction()?;
        let sent_at = chrono::Utc::now().timestamp();
        tx.execute(
            "INSERT INTO desktop_events(rule_id,stream_id,rule_name,user_name,title,url,sent_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                LEGACY_ID,
                stream.id,
                LEGACY_NAME,
                stream.user_name,
                stream.title,
                stream.url(),
                sent_at,
            ],
        )?;
        sessions::restore_sent_stream(&tx, stream, sent_at)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn observe(
        &mut self,
        rule: &Rule,
        streams: &[Stream],
        notify_existing: bool,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let key = rule.baseline_key();
        let tx = self.connection.transaction()?;
        sessions::observe(&tx, streams, now)?;
        Self::initialize_baseline(&tx, &key, streams, notify_existing)?;
        {
            let mut baseline =
                tx.prepare("SELECT 1 FROM baselines WHERE filter_key=?1 AND stream_id=?2")?;
            let mut insert = tx.prepare("INSERT OR IGNORE INTO desktop_events(rule_id,stream_id,rule_name,user_name,title,url,snapshot)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)")?;
            for stream in streams {
                if baseline
                    .query_row(params![key, stream.id], |_| Ok(()))
                    .optional()?
                    .is_some()
                {
                    continue;
                }
                if rule.channels.desktop {
                    insert.execute(params![
                        rule.id,
                        stream.id,
                        rule.name,
                        stream.user_name,
                        stream.title,
                        stream.url(),
                        serde_json::to_string(stream)?
                    ])?;
                }
                if rule.channels.email {
                    crate::mail::enqueue(
                        &tx,
                        &rule.id,
                        &rule.name,
                        &rule.filter.category,
                        stream,
                        &rule.mail,
                        now,
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn pending_desktop(&self, rule_id: &str) -> Result<Vec<(String, Stream)>> {
        let mut stmt = self.connection.prepare("SELECT rule_name,snapshot FROM desktop_events WHERE rule_id=?1 AND sent_at IS NULL ORDER BY rowid LIMIT 100")?;
        let rows = stmt.query_map([rule_id], |r| Ok((r.get(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|row| {
            let (name, json) = row?;
            Ok((name, serde_json::from_str(&json)?))
        })
        .collect()
    }

    pub(crate) fn confirm_desktop(&mut self, rule_id: &str, stream_id: &str) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sent_at = chrono::Utc::now().timestamp();
        sessions::restore_pending_desktop(&tx, rule_id, stream_id, sent_at)?;
        tx.execute("UPDATE desktop_events SET sent_at=?3,snapshot=NULL WHERE rule_id=?1 AND stream_id=?2 AND sent_at IS NULL", params![rule_id,stream_id,sent_at])?;
        tx.commit()?;
        Ok(())
    }

    pub fn recent_history(&self) -> Result<Vec<Delivery>> {
        history_items(&self.connection, "", 0, 20)
    }

    pub(crate) fn history_at(path: &Path, query: &str, offset: u32, limit: u32) -> Result<History> {
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        connection.busy_timeout(Duration::from_secs(3))?;
        connection.create_scalar_function(
            "unicode_lower",
            1,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |args| Ok(args.get::<String>(0)?.to_lowercase()),
        )?;
        let transaction = connection.transaction()?;
        let query = query.trim().to_lowercase();
        let total: i64 = transaction.query_row(
            &format!(
                "SELECT count(*) FROM alert_history WHERE 1=1 {}",
                history_filter(&query)
            ),
            rusqlite::params_from_iter((!query.is_empty()).then_some(&query)),
            |row| row.get(0),
        )?;
        let items = history_items(&transaction, &query, offset, limit)?;
        transaction.commit()?;
        Ok(History {
            items,
            total: u64::try_from(total)?,
        })
    }
}

fn history_filter(query: &str) -> &'static str {
    if query.is_empty() {
        ""
    } else {
        "AND (instr(unicode_lower(rule_name), ?1) > 0 OR instr(unicode_lower(coalesce(user_name,'Chaîne inconnue')), ?1) > 0 OR instr(unicode_lower(coalesce(title,'')), ?1) > 0 OR instr(unicode_lower(substr(coalesce(url,''), length('https://www.twitch.tv/') + 1)), ?1) > 0)"
    }
}

fn history_items(
    connection: &Connection,
    query: &str,
    offset: u32,
    limit: u32,
) -> Result<Vec<Delivery>> {
    let sql = format!(
        "SELECT stream_id,user_name,title,url,recorded_at,rule_id,rule_name,channel
         FROM alert_history WHERE 1=1 {}
         ORDER BY recorded_at DESC, tie DESC, channel, rule_id, stream_id LIMIT ?2 OFFSET ?3",
        history_filter(query)
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![query, limit, offset], |row| {
        Ok(Delivery {
            rule_id: row.get(5)?,
            rule_name: row.get(6)?,
            channel: row.get(7)?,
            stream_id: row.get(0)?,
            user_name: row.get(1)?,
            title: row.get(2)?,
            url: row.get(3)?,
            recorded_at: row.get::<_, i64>(4)?.max(0) as u64,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn keeps_legacy_deliveries_and_baseline_when_migrating() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("alerts.sqlite3");
        let legacy = Connection::open(&path)?;
        legacy.execute_batch(
            "CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE deliveries (
                 stream_id TEXT PRIMARY KEY,
                 disposition TEXT NOT NULL CHECK(disposition IN ('sent', 'baseline')),
                 message_id TEXT,
                 recorded_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             INSERT INTO metadata VALUES ('initialized', 'true');
             INSERT INTO deliveries(stream_id, disposition, recorded_at) VALUES
                 ('1', 'sent', 123), ('2', 'baseline', 124);
             PRAGMA user_version=1;",
        )?;
        drop(legacy);
        let store = Store::open(&path)?;
        assert!(store.contains("1", &StreamFilter::default())?);
        assert!(store.contains("2", &StreamFilter::default())?);
        assert_eq!(store.recent_history()?.len(), 1);
        let legacy = &store.recent_history()?[0];
        assert_eq!(legacy.recorded_at, 123);
        assert!(legacy.url.is_empty());
        let mut sessions = store.connection.prepare(
            "SELECT first_seen_at, last_seen_at FROM stream_sessions ORDER BY stream_id",
        )?;
        let times: Vec<(i64, Option<i64>)> = sessions
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(times, [(123, None), (124, Some(124))]);
        Ok(())
    }
}
