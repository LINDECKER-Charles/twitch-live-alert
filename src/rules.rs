use crate::{mail::MailPolicy, schedule::Schedule, twitch::StreamFilter};
use serde::{Deserialize, Serialize};

pub(crate) const LEGACY_ID: &str = "legacy";
pub(crate) const LEGACY_NAME: &str = "Surveillance initiale";
pub(crate) const MAX_RULES: usize = 64;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Channels {
    pub desktop: bool,
    pub email: bool,
}

impl Default for Channels {
    fn default() -> Self {
        Self {
            desktop: true,
            email: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub enabled: bool,
    #[serde(flatten)]
    pub filter: StreamFilter,
    #[serde(default)]
    pub box_art_url: String,
    #[serde(default)]
    pub channels: Channels,
    #[serde(default)]
    pub detection: Schedule,
    #[serde(default)]
    pub mail: MailPolicy,
}

impl Rule {
    pub fn legacy(filter: StreamFilter) -> Self {
        Self {
            id: LEGACY_ID.into(),
            name: LEGACY_NAME.into(),
            enabled: true,
            filter,
            box_art_url: String::new(),
            channels: Channels::default(),
            detection: Schedule::default(),
            mail: MailPolicy::default(),
        }
    }

    pub fn validate(&self) -> bool {
        !self.name.trim().is_empty()
            && self.name.chars().count() <= 100
            && !self.name.chars().any(char::is_control)
            && self.filter.valid()
            && (self.channels.desktop || self.channels.email)
            && self.detection.validate().is_ok()
            && self.mail.validate().is_ok()
    }

    pub fn baseline_key(&self) -> String {
        if self.id == LEGACY_ID {
            self.filter.baseline_key()
        } else {
            format!("{}:{}", self.id, self.filter.baseline_key())
        }
    }
}
