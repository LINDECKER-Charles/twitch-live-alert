//! Local SMTP settings, durable delivery and shared rule scheduling.
mod api;
mod delivery;
mod outbox;
mod settings;
mod template;
mod transport;

use crate::schedule::{Schedule, time};
use anyhow::{Result, ensure};
pub use outbox::{enqueue, migrate};
pub(crate) use outbox::{reconcile_rules, retire_rule};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MailMode {
    #[default]
    Instant,
    Window,
    Digest,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MailPolicy {
    pub mode: MailMode,
    pub schedule: Schedule,
    pub digest_times: Vec<String>,
}

impl MailPolicy {
    pub fn validate(&self) -> Result<()> {
        self.schedule.validate()?;
        ensure!(
            self.digest_times.len() <= 24,
            "Limite de 24 horaires de récapitulatif."
        );
        let times = self
            .digest_times
            .iter()
            .map(|s| time(s))
            .collect::<Result<Vec<_>>>()?;
        if self.mode == MailMode::Digest {
            ensure!(
                times
                    .iter()
                    .any(|&t| (1..=7).any(|day| self.schedule.allows_wall_time(day, t))),
                "Aucun horaire de récapitulatif dans les plages choisies."
            );
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct MailService {
    #[cfg(test)]
    test_ca: Option<lettre::transport::smtp::client::Certificate>,
    profile: PathBuf,
    database: Option<PathBuf>,
    eligible: Option<Arc<dyn Fn() -> Vec<String> + Send + Sync>>,
    changes: Arc<Mutex<()>>,
}

impl MailService {
    pub fn new(profile_dir: PathBuf) -> Self {
        Self {
            #[cfg(test)]
            test_ca: None,
            profile: profile_dir,
            database: None,
            eligible: None,
            changes: Arc::new(Mutex::new(())),
        }
    }

    pub fn with_database(mut self, path: Option<PathBuf>) -> Self {
        self.database = path;
        self
    }

    pub(crate) fn with_eligibility(
        mut self,
        ids: impl Fn() -> Vec<String> + Send + Sync + 'static,
    ) -> Self {
        self.eligible = Some(Arc::new(ids));
        self
    }

    fn file(&self) -> PathBuf {
        self.profile.join("mail.json")
    }

    /// Claims at most one message. The caller can drain a bounded batch on its
    /// own task, independently of Twitch, and stop between messages.
    pub async fn dispatch_next(
        &self,
        db_path: &Path,
        now: chrono::DateTime<chrono::Utc>,
        active_rule_ids: &[String],
    ) -> Result<bool> {
        outbox::dispatch(self, db_path, now, active_rule_ids).await
    }
}

#[cfg(test)]
mod tests;
