use crate::{
    config::Config,
    rules::{MAX_RULES, Rule},
    twitch::StreamFilter,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path, time::Duration};
use tracing::warn;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preferences {
    pub poll_interval_seconds: u64,
    pub notify_existing_on_first_run: bool,
    pub start_with_windows: bool,
    #[serde(flatten)]
    pub filter: StreamFilter,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Settings {
    pub(super) preferences: Preferences,
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub(super) disconnected: bool,
}

impl Settings {
    pub(super) fn defaults(config: Option<&Config>) -> Self {
        Self {
            preferences: Preferences {
                poll_interval_seconds: config.map_or(60, |value| value.poll_interval.as_secs()),
                notify_existing_on_first_run: config.is_some_and(|value| value.notify_existing),
                start_with_windows: false,
                filter: StreamFilter::default(),
            },
            enabled: true,
            rules: None,
            disconnected: false,
        }
    }

    pub(super) fn load(path: &Path, defaults: Self) -> Self {
        match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Self>(&bytes) {
                Ok(settings) if settings.valid() => settings,
                _ => {
                    warn!("Réglages locaux invalides, valeurs par défaut utilisées");
                    defaults
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => defaults,
            Err(_) => {
                warn!("Réglages locaux illisibles, valeurs par défaut utilisées");
                defaults
            }
        }
    }

    pub(super) fn rules(&self) -> Vec<Rule> {
        self.rules
            .clone()
            .unwrap_or_else(|| vec![Rule::legacy(self.preferences.filter.clone())])
    }

    pub(super) fn active_count(&self) -> usize {
        self.rules
            .as_deref()
            .map_or(1, |rules| rules.iter().filter(|rule| rule.enabled).count())
    }

    fn valid(&self) -> bool {
        (15..=3600).contains(&self.preferences.poll_interval_seconds)
            && self.preferences.filter.valid()
            && self
                .rules
                .as_ref()
                .is_none_or(|rules| rules.len() <= MAX_RULES && rules.iter().all(Rule::validate))
    }

    pub(super) fn save(&self, path: &Path) -> Result<()> {
        let directory = path.parent().context("Chemin de réglages invalide")?;
        fs::create_dir_all(directory)?;
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer_pretty(&mut file, &self)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }

    pub(super) fn interval(&self) -> Duration {
        Duration::from_secs(self.preferences.poll_interval_seconds)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreferencesUpdate {
    pub poll_interval_seconds: u64,
    pub notify_existing_on_first_run: bool,
    pub start_with_windows: bool,
    pub category: Option<String>,
    pub language: Option<String>,
}

impl PreferencesUpdate {
    pub(super) fn merge(self, previous: &Preferences) -> Preferences {
        Preferences {
            poll_interval_seconds: self.poll_interval_seconds,
            notify_existing_on_first_run: self.notify_existing_on_first_run,
            start_with_windows: self.start_with_windows,
            filter: StreamFilter {
                category: self
                    .category
                    .unwrap_or_else(|| previous.filter.category.clone()),
                language: self
                    .language
                    .unwrap_or_else(|| previous.filter.language.clone()),
            },
        }
    }
}
