use anyhow::{Context, Result, bail};
use std::{env, path::PathBuf, time::Duration};

// Deliberately no Debug: credentials must never end up in logs.
pub struct Config {
    pub client_id: String,
    pub client_secret: String,
    pub poll_interval: Duration,
    pub database_path: PathBuf,
    pub notify_existing: bool,
}

#[derive(Clone, Copy)]
pub enum Mode {
    Watch,
    DryRun,
    TestNotification,
}

impl Config {
    pub fn from_env(mode: Mode) -> Result<Self> {
        Self::from_lookup(mode, |key| env::var(key).ok())
    }

    fn from_lookup(mode: Mode, get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let required = |key| -> Result<String> {
            let value = get(key).unwrap_or_default();
            if value.trim().is_empty() {
                bail!(
                    "Variable {key} manquante. Voir README.md pour la création des identifiants."
                );
            }
            Ok(value.trim().to_owned())
        };
        let (client_id, client_secret) = if matches!(mode, Mode::TestNotification) {
            (String::new(), String::new())
        } else {
            (
                required("TWITCH_CLIENT_ID")?,
                required("TWITCH_CLIENT_SECRET")?,
            )
        };
        let seconds: u64 = get("POLL_INTERVAL_SECONDS")
            .unwrap_or_else(|| "60".into())
            .parse()
            .context("POLL_INTERVAL_SECONDS doit être un entier entre 15 et 3600")?;
        if !(15..=3600).contains(&seconds) {
            bail!("POLL_INTERVAL_SECONDS doit être compris entre 15 et 3600");
        }
        let notify_existing = match get("NOTIFY_EXISTING_ON_FIRST_RUN").as_deref() {
            None | Some("true") => true,
            Some("false") => false,
            _ => bail!("NOTIFY_EXISTING_ON_FIRST_RUN doit valoir true ou false"),
        };
        let database_path =
            PathBuf::from(get("DATABASE_PATH").unwrap_or_else(|| "data/alerts.sqlite3".into()));
        if database_path.as_os_str().is_empty() || database_path.to_str() == Some(":memory:") {
            bail!("DATABASE_PATH doit désigner un fichier persistant");
        }
        Ok(Self {
            client_id,
            client_secret,
            poll_interval: Duration::from_secs(seconds),
            database_path,
            notify_existing,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    fn vars() -> HashMap<&'static str, String> {
        HashMap::from([
            ("TWITCH_CLIENT_ID", "test-client".into()),
            ("TWITCH_CLIENT_SECRET", "secret-do-not-print".into()),
        ])
    }
    #[test]
    fn twitch_is_the_only_required_service() {
        let vars = vars();
        for mode in [Mode::Watch, Mode::DryRun] {
            let config = Config::from_lookup(mode, |key| vars.get(key).cloned()).unwrap();
            assert_eq!(config.poll_interval, Duration::from_secs(60));
        }
        assert!(Config::from_lookup(Mode::TestNotification, |_| None).is_ok());
    }
    #[test]
    fn rejects_invalid_runtime_settings() {
        for (key, value) in [
            ("POLL_INTERVAL_SECONDS", "0"),
            ("POLL_INTERVAL_SECONDS", "3601"),
            ("NOTIFY_EXISTING_ON_FIRST_RUN", "yes"),
            ("DATABASE_PATH", ":memory:"),
        ] {
            let mut vars = vars();
            vars.insert(key, value.into());
            assert!(Config::from_lookup(Mode::Watch, |key| vars.get(key).cloned()).is_err());
        }
    }
}
