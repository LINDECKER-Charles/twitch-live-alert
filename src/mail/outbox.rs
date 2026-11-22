use super::{
    MailMode, MailPolicy, MailService,
    settings::Stored,
    template::{self, Context, Rendered},
};
use crate::twitch::Stream;
use anyhow::{Result, ensure};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::path::Path;

pub fn migrate(db: &Connection) -> Result<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS mail_batches(
            id TEXT PRIMARY KEY NOT NULL DEFAULT(lower(hex(randomblob(16)))),
            rule_id TEXT NOT NULL, digest_slot INTEGER, policy TEXT NOT NULL,
            state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','sending','sent')),
            next_attempt INTEGER NOT NULL, attempts INTEGER NOT NULL DEFAULT 0,
            sent_at INTEGER, last_error TEXT, failed_at INTEGER,
            UNIQUE(rule_id,digest_slot)
        );
        CREATE INDEX IF NOT EXISTS mail_due ON mail_batches(state,next_attempt);
        CREATE TABLE IF NOT EXISTS mail_events(
            rule_id TEXT NOT NULL, stream_id TEXT NOT NULL, rule_name TEXT NOT NULL,
            user_name TEXT NOT NULL, title TEXT NOT NULL, url TEXT NOT NULL,
            batch_id TEXT NOT NULL REFERENCES mail_batches(id), context TEXT NOT NULL,
            PRIMARY KEY(rule_id,stream_id)
        );
        CREATE INDEX IF NOT EXISTS mail_members ON mail_events(batch_id);",
    )?;
    let has_time: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('mail_batches') WHERE name='failed_at')",
        [],
        |r| r.get(0),
    )?;
    if !has_time {
        db.execute_batch("ALTER TABLE mail_batches ADD COLUMN failed_at INTEGER;")?;
    }
    Ok(())
}

/// Joins the caller's SQLite transaction so a detected event cannot be recorded
/// without its mail delivery. Standalone callers receive the same atomicity.
pub fn enqueue(
    db: &Connection,
    rule_id: &str,
    rule_name: &str,
    category: &str,
    stream: &Stream,
    policy: &MailPolicy,
    now: DateTime<Utc>,
) -> Result<()> {
    if db.is_autocommit() {
        let tx = rusqlite::Transaction::new_unchecked(db, TransactionBehavior::Immediate)?;
        enqueue(&tx, rule_id, rule_name, category, stream, policy, now)?;
        tx.commit()?;
        return Ok(());
    }
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM mail_events WHERE rule_id=? AND stream_id=?)",
        params![rule_id, stream.id],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(());
    }

    policy.validate()?;
    let mut due = match policy.mode {
        MailMode::Instant => now,
        MailMode::Window => policy.schedule.next_window(now)?,
        MailMode::Digest => policy.schedule.next_digest(&policy.digest_times, now)?,
    };
    let still_open = if policy.mode == MailMode::Digest {
        loop {
            let existing = db
                .query_row(
                    "SELECT id,attempts FROM mail_batches WHERE rule_id=? AND digest_slot=?",
                    params![rule_id, due.timestamp()],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
                )
                .optional()?;
            match existing {
                None => break None,
                Some((id, 0)) => break Some(id),
                // Never mutate a message whose delivery has already started.
                Some(_) => {
                    due = policy
                        .schedule
                        .next_digest(&policy.digest_times, due + Duration::seconds(1))?
                }
            }
        }
    } else {
        None
    };
    let id = match still_open {
        Some(id) => id,
        None => db.query_row(
            "INSERT INTO mail_batches(rule_id,digest_slot,policy,next_attempt) VALUES(?,?,?,?) RETURNING id",
            params![rule_id,(policy.mode == MailMode::Digest).then(|| due.timestamp()),serde_json::to_string(policy)?,due.timestamp()],
            |r| r.get(0)
        )?,
    };
    let context = serde_json::to_string(&Context::new(rule_name, category, stream, now))?;
    db.execute(
        "INSERT INTO mail_events(rule_id,stream_id,rule_name,user_name,title,url,batch_id,context) VALUES(?,?,?,?,?,?,?,?)",
        params![rule_id,stream.id,rule_name,stream.user_name,stream.title,stream.url(),id,context]
    )?;
    Ok(())
}

struct Job {
    id: String,
    attempt: u32,
    mode: MailMode,
    items: Vec<Context>,
}

impl Job {
    fn render(&self, settings: &Stored) -> Result<Rendered> {
        let config = &settings.settings;
        let mut rendered = self
            .items
            .iter()
            .map(|c| template::render(&config.subject_template, &config.body_template, c));
        let mut message = rendered
            .next()
            .ok_or_else(|| anyhow::anyhow!("Récapitulatif vide."))??;
        if self.mode == MailMode::Digest {
            message.subject = format!(
                "[Récap · {} direct{}] {}",
                self.items.len(),
                if self.items.len() > 1 { "s" } else { "" },
                message.subject
            );
        }
        for next in rendered {
            message.body.push_str("\n\n──────────\n\n");
            message.body.push_str(&next?.body);
        }
        Ok(message)
    }
}

enum Claim {
    Empty,
    Rescheduled,
    Ready(Job),
}

fn connect(path: &Path) -> Result<Connection> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(db)
}

fn claim(path: &Path, now: DateTime<Utc>, rules: &[String]) -> Result<Claim> {
    let mut db = connect(path)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let row = tx.query_row(
        "SELECT id,policy,attempts FROM mail_batches WHERE state IN ('pending','sending')
        AND next_attempt <= ? AND rule_id IN (SELECT value FROM json_each(?)) ORDER BY next_attempt,rowid LIMIT 1",
        params![now.timestamp(),serde_json::to_string(rules)?],
        |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,u32>(2)?))
    ).optional()?;
    let Some((id, policy, previous)) = row else {
        return Ok(Claim::Empty);
    };
    let policy: MailPolicy = serde_json::from_str(&policy)?;
    if policy.mode == MailMode::Window && !policy.schedule.allows(now) {
        let next = policy.schedule.next_window(now)?;
        tx.execute(
            "UPDATE mail_batches SET state='pending',next_attempt=? WHERE id=?",
            params![next.timestamp(), id],
        )?;
        tx.commit()?;
        return Ok(Claim::Rescheduled);
    }
    // The lease exceeds the total network timeout. A crashed process releases
    // its SQLite lock automatically and a later worker can reclaim the message.
    let attempt = previous.saturating_add(1);
    tx.execute(
        "UPDATE mail_batches SET state='sending',attempts=?,next_attempt=? WHERE id=?",
        params![attempt, now.timestamp() + 600, id],
    )?;
    let items = {
        let mut stmt =
            tx.prepare("SELECT context FROM mail_events WHERE batch_id=? ORDER BY rowid")?;
        let rows = stmt.query_map([&id], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?))
            .collect::<Result<Vec<Context>>>()?
    };
    tx.commit()?;
    Ok(Claim::Ready(Job {
        id,
        attempt,
        mode: policy.mode,
        items,
    }))
}

fn finish(
    path: &Path,
    id: &str,
    attempt: u32,
    now: DateTime<Utc>,
    error: Option<String>,
) -> Result<()> {
    let db = connect(path)?;
    let count = match error {
        None => db.execute("UPDATE mail_batches SET state='sent',sent_at=?,last_error=NULL,failed_at=NULL WHERE id=? AND state='sending' AND attempts=?",params![now.timestamp(),id,attempt])?,
        Some(error) => {
            let backoff = (30 * 2_i64.pow(attempt.saturating_sub(1).min(7))).min(3600);
            db.execute("UPDATE mail_batches SET state='pending',next_attempt=?,last_error=?,failed_at=? WHERE id=? AND state='sending' AND attempts=?",params![now.timestamp()+backoff,error,now.timestamp(),id,attempt])?
        }
    };
    ensure!(count == 1, "Le bail de la livraison SMTP a expiré.");
    Ok(())
}

pub(super) async fn dispatch(
    service: &MailService,
    path: &Path,
    now: DateTime<Utc>,
    rules: &[String],
) -> Result<bool> {
    if rules.is_empty() {
        return Ok(false);
    }
    let db = path.to_owned();
    let names = rules.to_vec();
    let claim = tokio::task::spawn_blocking(move || claim(&db, now, &names)).await??;
    let job = match claim {
        Claim::Empty => return Ok(false),
        Claim::Rescheduled => return Ok(true),
        Claim::Ready(job) => job,
    };
    let started = std::time::Instant::now();
    let settings_path = service.file();
    let settings = tokio::task::spawn_blocking(move || Stored::load(&settings_path)).await?;
    let delivery = async {
        let settings = settings?;
        let rendered = job.render(&settings)?;
        service.send(&settings, Some(&job.id), rendered).await
    }
    .await;
    let finished = now + Duration::from_std(started.elapsed())?;
    let error = delivery.err().map(|e| e.to_string());
    if let Some(reason) = &error {
        tracing::warn!(
            attempt = job.attempt,
            "Nouvelle tentative de livraison SMTP prévue : {reason}"
        );
    }
    let db = path.to_owned();
    tokio::task::spawn_blocking(move || finish(&db, &job.id, job.attempt, finished, error))
        .await??;
    Ok(true)
}
