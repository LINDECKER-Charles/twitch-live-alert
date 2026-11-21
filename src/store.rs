use crate::twitch::{Stream, StreamFilter};
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags, OptionalExtension, functions::FunctionFlags, params};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    time::Duration,
};

pub struct Store {
    connection: Connection,
    _lock: File,
}

#[derive(Clone, Serialize)]
pub struct Delivery {
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
        let connection = Connection::open(path).context("Impossible d'ouvrir la base SQLite")?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 3 {
            bail!("Version de base SQLite plus récente que ce service");
        }
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        if version < 2 {
            connection
                .execute_batch(
                    "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS deliveries (
                     stream_id TEXT PRIMARY KEY,
                     disposition TEXT NOT NULL CHECK(disposition IN ('sent', 'baseline')),
                     message_id TEXT,
                     recorded_at INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 ALTER TABLE deliveries ADD COLUMN user_name TEXT;
                 ALTER TABLE deliveries ADD COLUMN title TEXT;
                 ALTER TABLE deliveries ADD COLUMN url TEXT;
                 PRAGMA user_version=2;
                 COMMIT;",
                )
                .context("Impossible de migrer la base SQLite")?;
        }
        if version < 3 {
            connection.execute_batch(
                "BEGIN IMMEDIATE;
                 UPDATE metadata SET key='initialized:fr:bloodborne' WHERE key='initialized';
                 CREATE TABLE baselines (
                     filter_key TEXT NOT NULL,
                     stream_id TEXT NOT NULL,
                     PRIMARY KEY(filter_key, stream_id)
                 );
                 INSERT INTO baselines(filter_key, stream_id)
                     SELECT 'initialized:fr:bloodborne', stream_id FROM deliveries WHERE disposition='baseline';
                 DELETE FROM deliveries WHERE disposition='baseline';
                 CREATE INDEX delivery_dates ON deliveries(recorded_at DESC) WHERE disposition='sent';
                 PRAGMA user_version=3;
                 COMMIT;"
            ).context("Impossible de migrer les filtres de surveillance")?;
        }
        Ok(Self {
            connection,
            _lock: lock,
        })
    }

    pub fn initialize(
        &mut self,
        filter: &StreamFilter,
        streams: &[Stream],
        notify_existing: bool,
    ) -> Result<()> {
        let key = filter.baseline_key();
        let initialized: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM metadata WHERE key=?1)",
            [&key],
            |row| row.get(0),
        )?;
        if initialized {
            return Ok(());
        }
        let transaction = self.connection.transaction()?;
        if !notify_existing {
            for stream in streams {
                transaction.execute(
                    "INSERT OR IGNORE INTO baselines(filter_key, stream_id) VALUES (?1, ?2)",
                    params![key, stream.id],
                )?;
            }
        }
        transaction.execute("INSERT INTO metadata(key,value) VALUES (?1,'true')", [&key])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn contains(&self, stream_id: &str, filter: &StreamFilter) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM deliveries WHERE stream_id=?1
                 UNION ALL SELECT 1 FROM baselines WHERE stream_id=?1 AND filter_key=?2 LIMIT 1",
                params![stream_id, filter.baseline_key()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn mark_sent(&self, stream: &Stream) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO deliveries(stream_id, disposition, user_name, title, url)
             VALUES (?1, 'sent', ?2, ?3, ?4)",
                params![stream.id, stream.user_name, stream.title, stream.url()],
            )
            .context("Windows a accepté la notification, mais SQLite n'a pas pu l'enregistrer")?;
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
                "SELECT count(*) FROM deliveries WHERE disposition='sent' {}",
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
        "AND (instr(unicode_lower(coalesce(user_name,'Chaîne inconnue')), ?1) > 0 OR instr(unicode_lower(coalesce(title,'')), ?1) > 0 OR instr(unicode_lower(substr(coalesce(url,''), length('https://www.twitch.tv/') + 1)), ?1) > 0)"
    }
}

fn history_items(
    connection: &Connection,
    query: &str,
    offset: u32,
    limit: u32,
) -> Result<Vec<Delivery>> {
    let sql = format!(
        "SELECT stream_id, coalesce(user_name, 'Chaîne inconnue'), coalesce(title,''), coalesce(url,''), recorded_at
         FROM deliveries WHERE disposition='sent' {}
         ORDER BY recorded_at DESC, rowid DESC LIMIT ?2 OFFSET ?3", history_filter(query)
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![query, limit, offset], |row| {
        Ok(Delivery {
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
        Ok(())
    }
}
