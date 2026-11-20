use anyhow::{Context, Result, bail};
use reqwest::Url;
use std::{env, path::PathBuf, time::Duration};

// Deliberately no Debug: credentials must never end up in logs.
pub struct Config {
    pub client_id: String,
    pub client_secret: String,
    pub webhook: Option<Url>,
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
        let webhook = match get("DISCORD_WEBHOOK_URL").filter(|s| !s.trim().is_empty()) {
            Some(value) => Some(validate_webhook(value.trim())?),
            None if matches!(mode, Mode::DryRun) => None,
            None => bail!("Variable DISCORD_WEBHOOK_URL manquante. Voir README.md."),
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
            webhook,
            poll_interval: Duration::from_secs(seconds),
            database_path,
            notify_existing,
        })
    }
}

fn validate_webhook(value: &str) -> Result<Url> {
    let invalid = || {
        anyhow::anyhow!(
            "DISCORD_WEBHOOK_URL doit être une URL HTTPS de webhook Discord (salon texte)"
        )
    };
    let url = Url::parse(value).map_err(|_| invalid())?;
    let parts: Vec<_> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    let webhook_parts = if parts.first() == Some(&"api") && parts.get(1) == Some(&"v10") {
        &parts[2..]
    } else if parts.first() == Some(&"api") {
        &parts[1..]
    } else {
        &[]
    };
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("discord.com" | "canary.discord.com" | "ptb.discord.com")
        )
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || webhook_parts.len() != 3
        || webhook_parts[0] != "webhooks"
        || !webhook_parts[1].bytes().all(|b| b.is_ascii_digit())
        || !webhook_parts[2]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(invalid());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars() -> HashMap<&'static str, String> {
        HashMap::from([
            ("TWITCH_CLIENT_ID", "test-client".into()),
            ("TWITCH_CLIENT_SECRET", "secret-do-not-print".into()),
            (
                "DISCORD_WEBHOOK_URL",
                "https://discord.com/api/webhooks/123/test-token".into(),
            ),
        ])
    }

    #[test]
    fn defaults_and_dry_run_without_webhook() {
        let mut vars = vars();
        vars.remove("DISCORD_WEBHOOK_URL");
        let config = Config::from_lookup(Mode::DryRun, |key| vars.get(key).cloned()).unwrap();
        assert_eq!(config.poll_interval, Duration::from_secs(60));
        assert!(config.webhook.is_none());
        assert!(Config::from_lookup(Mode::Watch, |key| vars.get(key).cloned()).is_err());
    }

    #[test]
    fn rejects_bad_configuration_without_echoing_it() {
        for (key, value) in [
            ("POLL_INTERVAL_SECONDS", "0"),
            ("POLL_INTERVAL_SECONDS", "3601"),
            ("NOTIFY_EXISTING_ON_FIRST_RUN", "yes"),
            ("DISCORD_WEBHOOK_URL", "http://evil.invalid/private-secret"),
            (
                "DISCORD_WEBHOOK_URL",
                "https://discord.com/api/webhooks/123/private-secret?wait=false",
            ),
            ("DATABASE_PATH", ":memory:"),
        ] {
            let mut vars = vars();
            vars.insert(key, value.into());
            let error = Config::from_lookup(Mode::Watch, |key| vars.get(key).cloned())
                .err()
                .unwrap();
            assert!(!format!("{error:#}").contains("private-secret"));
        }
    }
}
