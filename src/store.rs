use crate::twitch::Stream;
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    time::Duration,
};

pub struct Store {
    connection: Connection,
    _lock: File,
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
        if version > 1 {
            bail!("Version de base SQLite plus récente que ce service");
        }
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS deliveries (
                 stream_id TEXT PRIMARY KEY,
                 disposition TEXT NOT NULL CHECK(disposition IN ('sent', 'baseline')),
                 message_id TEXT,
                 recorded_at INTEGER NOT NULL DEFAULT (unixepoch())
             );
             PRAGMA user_version=1;
             COMMIT;",
            )
            .context("Impossible d'initialiser la base SQLite")?;
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

    pub fn mark_sent(&self, stream_id: &str, message_id: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO deliveries(stream_id, disposition, message_id) VALUES (?1, 'sent', ?2)",
            params![stream_id, message_id]
        ).context("Discord a confirmé l'envoi, mais SQLite n'a pas pu l'enregistrer")?;
        Ok(())
    }
}
