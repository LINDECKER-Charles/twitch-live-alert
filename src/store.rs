use crate::twitch::Stream;
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use rusqlite::{Connection, OptionalExtension, params};
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
        if version > 2 {
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
        Ok(Self {
            connection,
            _lock: lock,
        })
    }

    pub fn initialize(&mut self, streams: &[Stream], notify_existing: bool) -> Result<()> {
        let initialized: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM metadata WHERE key='initialized')",
            [],
            |row| row.get(0),
        )?;
        if initialized {
            return Ok(());
        }
        let transaction = self.connection.transaction()?;
        if !notify_existing {
            for stream in streams {
                transaction.execute(
                    "INSERT OR IGNORE INTO deliveries(stream_id, disposition) VALUES (?1, 'baseline')", [&stream.id]
                )?;
            }
        }
        transaction.execute(
            "INSERT INTO metadata(key,value) VALUES ('initialized','true')",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn contains(&self, stream_id: &str) -> Result<bool> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM deliveries WHERE stream_id=?1",
                [stream_id],
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
        let mut statement = self.connection.prepare(
            "SELECT stream_id, COALESCE(user_name, 'Chaîne inconnue'),
                    COALESCE(title, ''), COALESCE(url, ''), recorded_at
             FROM deliveries WHERE disposition='sent'
             ORDER BY recorded_at DESC, rowid DESC LIMIT 20",
        )?;
        let rows = statement.query_map([], |row| {
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
        assert!(store.contains("1")?);
        assert!(store.contains("2")?);
        assert_eq!(store.recent_history()?.len(), 1);
        let legacy = &store.recent_history()?[0];
        assert_eq!(legacy.recorded_at, 123);
        assert!(legacy.url.is_empty());
        Ok(())
    }
}
