use crate::twitch::Stream;
use anyhow::{Result, ensure};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) const DEFAULT_SUBJECT: &str = "{{user_name}} est en direct sur Twitch";
pub(super) const DEFAULT_BODY: &str = "{{user_name}} joue à {{category}}\n{{title}}\n\n{{url}}\n\nLangue : {{language}} · Spectateurs : {{viewer_count}}\nDébut du direct : {{started_at}}\nRègle : {{rule}}";
pub(super) const VARIABLES: &[&str] = &[
    "user_name",
    "user_login",
    "user_id",
    "stream_id",
    "title",
    "category",
    "game_id",
    "language",
    "started_at",
    "viewer_count",
    "url",
    "tags",
    "is_mature",
    "thumbnail_url",
    "type",
    "rule",
    "detected_at",
];

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Context(BTreeMap<String, String>);

impl Context {
    pub(super) fn new(rule: &str, category: &str, stream: &Stream, now: DateTime<Utc>) -> Self {
        let game = if stream.game_name.trim().is_empty() {
            category
        } else {
            &stream.game_name
        };
        Self(
            [
                ("stream_id", stream.id.clone()),
                ("user_login", stream.user_login.clone()),
                ("user_name", stream.user_name.clone()),
                ("user_id", stream.user_id.clone()),
                ("title", stream.title.clone()),
                ("category", game.to_owned()),
                ("game_id", stream.game_id.clone()),
                ("language", stream.language.clone()),
                ("started_at", stream.started_at.clone()),
                ("viewer_count", stream.viewer_count.to_string()),
                ("url", stream.url()),
                ("tags", stream.tags.join(", ")),
                (
                    "is_mature",
                    match stream.is_mature {
                        Some(true) => "Oui",
                        Some(false) => "Non",
                        None => "",
                    }
                    .to_owned(),
                ),
                (
                    "thumbnail_url",
                    stream
                        .thumbnail_url
                        .replace("{width}", "640")
                        .replace("{height}", "360"),
                ),
                ("type", stream.kind.clone()),
                ("rule", rule.to_owned()),
                (
                    "detected_at",
                    now.to_rfc3339_opts(SecondsFormat::Secs, true),
                ),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        )
    }

    pub(super) fn sample() -> Self {
        let stream = Stream {
            id: "123456789".into(),
            user_id: "12345".into(),
            user_login: "chasseuse_exemple".into(),
            user_name: "Chasseuse Exemple".into(),
            title: "Première chasse à Yharnam".into(),
            game_name: "Bloodborne".into(),
            game_id: "460636".into(),
            language: "fr".into(),
            started_at: "2026-01-10T18:00:00Z".into(),
            viewer_count: 42,
            tags: vec!["Français".into(), "Découverte".into()],
            thumbnail_url: "https://example.invalid/apercu-{width}x{height}.jpg".into(),
            is_mature: Some(false),
            kind: "live".into(),
        };
        Self::new(
            "Bloodborne en français",
            "Bloodborne",
            &stream,
            DateTime::from_timestamp(1768068300, 0).unwrap(),
        )
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Templates {
    pub subject_template: String,
    pub body_template: String,
}

#[derive(Serialize)]
pub(super) struct Rendered {
    pub subject: String,
    pub body: String,
}

fn expand(mut input: &str, values: &Context) -> Result<String> {
    let mut out = String::with_capacity(input.len());
    while let Some(i) = input.find("{{") {
        let before = &input[..i];
        if let Some(literal) = before.strip_suffix('\\') {
            out.push_str(literal);
            out.push_str("{{");
            input = &input[i + 2..];
            continue;
        }
        out.push_str(before);
        let rest = &input[i + 2..];
        let end = rest
            .find("}}")
            .ok_or_else(|| anyhow::anyhow!("Variable incomplète dans le modèle."))?;
        let name = rest[..end].trim();
        ensure!(VARIABLES.contains(&name), "Variable de modèle inconnue.");
        let value = values.0.get(name).map_or("", |v| v.trim());
        if value.is_empty() {
            out.push('—');
        } else {
            out.extend(value.chars().map(|c| if c.is_control() { ' ' } else { c }));
        }
        input = &rest[end + 2..];
    }
    out.push_str(input);
    Ok(out)
}

pub(super) fn render(subject: &str, body: &str, context: &Context) -> Result<Rendered> {
    ensure!(
        !subject.trim().is_empty()
            && subject.chars().count() <= 240
            && !subject.chars().any(char::is_control),
        "L’objet doit tenir sur une ligne de 1 à 240 caractères."
    );
    ensure!(
        !body.trim().is_empty()
            && body.len() <= 16_384
            && !body
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t')),
        "Le corps du message est vide ou trop long."
    );
    Ok(Rendered {
        subject: expand(subject, context)?,
        body: expand(body, context)?,
    })
}
