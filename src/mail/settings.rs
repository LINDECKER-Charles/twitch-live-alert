use super::template::{self, DEFAULT_BODY, DEFAULT_SUBJECT};
use crate::credentials::protect;
use anyhow::{Context, Result, bail, ensure};
use lettre::message::Mailbox;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, net::IpAddr, path::Path};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Security {
    #[default]
    Tls,
    Starttls,
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Settings {
    pub host: String,
    pub port: u16,
    pub security: Security,
    pub sender: String,
    pub recipients: Vec<String>,
    pub subject_template: String,
    pub body_template: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 465,
            security: Security::Tls,
            sender: String::new(),
            recipients: vec![],
            subject_template: DEFAULT_SUBJECT.into(),
            body_template: DEFAULT_BODY.into(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        let dns = self.host.len() <= 253
            && self.host.split('.').all(|s| {
                !s.is_empty()
                    && s.len() <= 63
                    && !s.starts_with('-')
                    && !s.ends_with('-')
                    && s.bytes().all(|b| b == b'-' || b.is_ascii_alphanumeric())
            });
        ensure!(
            self.port > 0 && (dns || self.host.parse::<IpAddr>().is_ok()),
            "Serveur ou port SMTP invalide."
        );
        ensure!(
            !self.recipients.is_empty() && self.recipients.len() <= 32,
            "Choisissez entre 1 et 32 destinataires."
        );
        for address in std::iter::once(&self.sender).chain(&self.recipients) {
            ensure!(
                address.len() <= 512
                    && !address.chars().any(char::is_control)
                    && address.parse::<Mailbox>().is_ok(),
                "Adresse de courrier invalide."
            );
        }
        template::render(
            &self.subject_template,
            &self.body_template,
            &template::Context::sample(),
        )?;
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
pub(super) struct Auth {
    pub username: String,
    pub password: String,
}

impl Drop for Auth {
    fn drop(&mut self) {
        self.username.zeroize();
        self.password.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Update {
    #[serde(flatten)]
    pub settings: Settings,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Drop for Update {
    fn drop(&mut self) {
        self.username.zeroize();
        self.password.zeroize();
    }
}

#[derive(Default, Deserialize, Serialize)]
pub(super) struct Stored {
    pub settings: Settings,
    // Stored atomically with the server to which the user granted these credentials.
    encrypted_auth: Option<Vec<u8>>,
}

impl Stored {
    pub fn configured_auth(&self) -> bool {
        self.encrypted_auth.is_some()
    }

    pub fn auth(&self) -> Result<Option<Auth>> {
        let Some(encrypted) = &self.encrypted_auth else {
            return Ok(None);
        };
        let plain = Zeroizing::new(protect(encrypted, false)?);
        Ok(Some(serde_json::from_slice(&plain).map_err(|_| {
            anyhow::anyhow!("Identifiants SMTP illisibles.")
        })?))
    }

    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(content) => serde_json::from_slice(&content)
                .map_err(|_| anyhow::anyhow!("Réglages SMTP locaux illisibles.")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => bail!("Lecture des réglages SMTP impossible."),
        }
    }

    pub fn apply(&mut self, mut update: Update) -> Result<()> {
        update.settings.validate()?;
        match (update.username.as_deref(), update.password.as_deref()) {
            (None, None) => {
                ensure!(
                    !self.configured_auth()
                        || (self
                            .settings
                            .host
                            .eq_ignore_ascii_case(&update.settings.host)
                            && self.settings.port == update.settings.port
                            && self.settings.security == update.settings.security),
                    "Renseignez à nouveau les identifiants pour changer de serveur SMTP."
                );
            }
            (Some(""), Some("")) => self.encrypted_auth = None,
            (Some(name), Some(password))
                if !name.trim().is_empty()
                    && !password.is_empty()
                    && name.len() <= 512
                    && password.len() <= 4096
                    && !name.chars().chain(password.chars()).any(char::is_control) =>
            {
                let auth = Auth {
                    username: update.username.take().unwrap(),
                    password: update.password.take().unwrap(),
                };
                let plain = Zeroizing::new(serde_json::to_vec(&auth)?);
                self.encrypted_auth = Some(protect(&plain, true)?);
            }
            _ => bail!("Renseignez ensemble l’identifiant et le mot de passe SMTP."),
        }
        self.settings = std::mem::take(&mut update.settings);
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let dir = path
            .parent()
            .context("Répertoire des réglages SMTP invalide.")?;
        fs::create_dir_all(dir)?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer(&mut file, self)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }
}
