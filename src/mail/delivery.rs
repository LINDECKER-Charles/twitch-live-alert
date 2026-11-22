use super::MailService;
use axum::{Json, extract::State};
use chrono::Utc;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use std::{path::Path, time::Duration};

#[derive(Default, Serialize)]
pub(super) struct DeliveryStatus {
    available: bool,
    pending_count: i64,
    scheduled_count: i64,
    failed_count: i64,
    next_attempt_at: Option<i64>,
    rule_name: Option<String>,
    error_code: Option<&'static str>,
    last_error: Option<&'static str>,
}

// Never include a remote SMTP response, a local parse error, or account data in
// the dashboard. Internal or previously unknown diagnostics receive a safe label.
pub(super) fn diagnostic(reason: &str) -> (&'static str, &'static str) {
    match reason {
        "Le serveur SMTP refuse le message. Vérifiez les adresses et les identifiants." => (
            "refused",
            "Le serveur SMTP refuse le message. Vérifiez les adresses et les identifiants.",
        ),
        "Connexion TLS au serveur SMTP impossible. Vérifiez le certificat, le port et le mode TLS."
        | "Initialisation TLS impossible." => ("tls", "La connexion TLS au serveur SMTP a échoué."),
        "Le serveur SMTP est temporairement indisponible." => (
            "temporary",
            "Le serveur SMTP est temporairement indisponible.",
        ),
        "Délai de réponse du serveur SMTP dépassé." => {
            ("timeout", "Délai de réponse du serveur SMTP dépassé.")
        }
        "Connexion SMTP impossible. Vérifiez le serveur, le port et le mode TLS." => (
            "connection",
            "Connexion SMTP impossible. Vérifiez le serveur, le port et le mode TLS.",
        ),
        _ => (
            "configuration",
            "Vérifiez les réglages du courrier et les identifiants SMTP.",
        ),
    }
}

impl DeliveryStatus {
    fn read(path: &Path, now: i64, eligible: Option<Vec<String>>) -> anyhow::Result<Self> {
        let mut db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(Duration::from_secs(2))?;
        let tx = db.transaction()?;
        let eligible = eligible
            .map(|ids| serde_json::to_string(&ids))
            .transpose()?;
        let mut status = tx.query_row(
            "SELECT count(*), count(*) FILTER(WHERE state='pending' AND attempts=0 AND next_attempt>?),
             count(*) FILTER(WHERE last_error IS NOT NULL), min(next_attempt)
             FROM mail_batches WHERE state IN ('pending','sending') AND rule_id NOT IN (SELECT rule_id FROM mail_retired_rules)
             AND (?2 IS NULL OR rule_id IN (SELECT value FROM json_each(?2)))", rusqlite::params![now,eligible],
            |r| Ok(Self { available:true,pending_count:r.get(0)?,scheduled_count:r.get(1)?,
                failed_count:r.get(2)?,next_attempt_at:r.get(3)?,..Self::default() })
        )?;
        if status.failed_count > 0 {
            let last = tx.query_row(
                "SELECT b.last_error,b.next_attempt,(SELECT e.rule_name FROM mail_events e WHERE e.batch_id=b.id ORDER BY rowid LIMIT 1)
                FROM mail_batches b WHERE b.state IN ('pending','sending') AND b.last_error IS NOT NULL
                AND b.rule_id NOT IN (SELECT rule_id FROM mail_retired_rules)
                AND (?1 IS NULL OR b.rule_id IN (SELECT value FROM json_each(?1)))
                ORDER BY b.failed_at DESC, b.rowid DESC LIMIT 1", [eligible],
                |r| Ok((r.get::<_,String>(0)?,r.get(1)?,r.get(2)?))
            ).optional()?;
            if let Some((reason, next, name)) = last {
                let (code, message) = diagnostic(&reason);
                status.error_code = Some(code);
                status.last_error = Some(message);
                status.rule_name = name;
                status.next_attempt_at = Some(next);
            }
        }
        tx.commit()?;
        Ok(status)
    }
}

pub(super) async fn get(State(service): State<MailService>) -> Json<DeliveryStatus> {
    let Some(path) = service.database else {
        return Json(DeliveryStatus::default());
    };
    let eligible = service.eligible.map(|ids| ids());
    Json(
        tokio::task::spawn_blocking(move || {
            DeliveryStatus::read(&path, Utc::now().timestamp(), eligible).unwrap_or_default()
        })
        .await
        .unwrap_or_default(),
    )
}
