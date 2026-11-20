use crate::{http, twitch::Stream};
use anyhow::Result;
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::{Value, json};

pub struct Discord {
    client: Client,
    webhook: Url,
}

impl Discord {
    pub fn new(client: Client, webhook: Url) -> Self {
        Self { client, webhook }
    }

    async fn post(&self, payload: Value) -> Result<String> {
        let response = self
            .client
            .post(self.webhook.clone())
            .query(&[("wait", "true")])
            .json(&payload)
            .send()
            .await
            .map_err(|e| http::transport(e, "Discord"))?;
        #[derive(Deserialize)]
        struct Message {
            id: String,
        }
        let message: Message =
            http::json(http::checked(response, "Discord").await?, "Discord").await?;
        if message.id.is_empty() {
            return Err(http::protocol("Discord", "confirmation de message absente"));
        }
        Ok(message.id)
    }

    pub async fn notify(&self, stream: &Stream) -> Result<String> {
        self.post(payload(stream)).await
    }

    pub async fn test_notification(&self) -> Result<()> {
        self.post(json!({
            "username": "Bloodborne FR Live",
            "content": "🔔 Test réussi : les alertes Bloodborne en français arriveront dans ce salon.",
            "allowed_mentions": { "parse": [] }
        })).await?;
        Ok(())
    }
}

pub(crate) fn payload(stream: &Stream) -> Value {
    json!({
        "username": "Bloodborne FR Live",
        "content": format!("🔴 {} joue à Bloodborne en français !\n{}", safe_text(&stream.user_name, 100), stream.url()),
        "allowed_mentions": { "parse": [] },
        "embeds": [{
            "title": "Voir le live sur Twitch",
            "url": stream.url(),
            "description": safe_text(&stream.title, 2000),
            "color": 0x9146ff,
            "fields": [
                { "name": "Langue déclarée", "value": "Français", "inline": true },
                { "name": "Spectateurs à la détection", "value": stream.viewer_count.to_string(), "inline": true }
            ],
            "footer": { "text": "Une alerte par session · Bloodborne FR Live" }
        }]
    })
}

// Escape streamer-controlled Markdown; keep Discord's UTF-16 limits including escapes.
fn safe_text(input: &str, max_units: usize) -> String {
    let mut text = String::new();
    let mut units = 0;
    for ch in input.chars().filter(|c| !c.is_control()) {
        let escape = matches!(
            ch,
            '\\' | '*' | '_' | '~' | '`' | '|' | '>' | '[' | ']' | '(' | ')'
        );
        let needed = ch.len_utf16() + usize::from(escape);
        if units + needed > max_units {
            break;
        }
        if escape {
            text.push('\\');
        }
        text.push(ch);
        units += needed;
    }
    if text.is_empty() {
        "Live Bloodborne".into()
    } else {
        text
    }
}
