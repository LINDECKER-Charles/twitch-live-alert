use crate::http;
use anyhow::Result;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, de::DeserializeOwned};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

const API: &str = "https://api.twitch.tv/helix";
const OAUTH: &str = "https://id.twitch.tv/oauth2";

#[derive(Clone, Debug, Deserialize)]
pub struct Stream {
    pub id: String,
    pub user_login: String,
    pub user_name: String,
    pub game_id: String,
    pub language: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub started_at: String,
    pub viewer_count: u64,
}

impl Stream {
    pub fn matches(&self, game_id: &str) -> bool {
        self.kind == "live" && self.game_id == game_id && self.language == "fr"
    }

    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || self.user_login.is_empty()
            || !self
                .user_login
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(http::protocol(
                "Twitch API",
                "identifiant de stream ou de chaîne invalide",
            ));
        }
        Ok(())
    }

    pub fn url(&self) -> String {
        format!("https://www.twitch.tv/{}", self.user_login)
    }
}

#[derive(Deserialize)]
struct Page<T> {
    data: Vec<T>,
    #[serde(default)]
    pagination: Pagination,
}
#[derive(Default, Deserialize)]
struct Pagination {
    cursor: Option<String>,
}
#[derive(Deserialize)]
struct Game {
    id: String,
    name: String,
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}
#[derive(Deserialize)]
struct Validation {
    client_id: String,
    expires_in: u64,
}
struct Token {
    value: String,
    expires_at: Instant,
    validated_at: Option<Instant>,
}

pub struct Twitch {
    client: Client,
    client_id: String,
    client_secret: String,
    api: String,
    oauth: String,
    token: Option<Token>,
    game_id: Option<String>,
}

impl Twitch {
    pub fn new(client: Client, client_id: String, client_secret: String) -> Self {
        Self {
            client,
            client_id,
            client_secret,
            api: API.into(),
            oauth: OAUTH.into(),
            token: None,
            game_id: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(client: Client, base: &str) -> Self {
        let mut twitch = Self::new(client, "test-client".into(), "test-secret".into());
        twitch.api = format!("{base}/helix");
        twitch.oauth = format!("{base}/oauth2");
        twitch
    }

    async fn acquire_token(&mut self) -> Result<()> {
        self.token = None;
        let response = self
            .client
            .post(format!("{}/token", self.oauth))
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await
            .map_err(|e| http::transport(e, "Twitch OAuth"))?;
        let data: TokenResponse = http::json(
            http::checked(response, "Twitch OAuth").await?,
            "Twitch OAuth",
        )
        .await?;
        if data.access_token.is_empty() || data.expires_in <= 60 {
            return Err(http::protocol(
                "Twitch OAuth",
                "jeton absent ou durée de validité insuffisante",
            ));
        }
        let now = Instant::now();
        let expires_at = now
            .checked_add(Duration::from_secs(data.expires_in))
            .ok_or_else(|| http::protocol("Twitch OAuth", "durée de validité invalide"))?;
        self.token = Some(Token {
            value: data.access_token,
            expires_at,
            validated_at: None,
        });
        Ok(())
    }

    async fn validate_token(&mut self) -> Result<()> {
        let token = self.token.as_ref().expect("token acquired");
        let response = self
            .client
            .get(format!("{}/validate", self.oauth))
            .bearer_auth(&token.value)
            .send()
            .await
            .map_err(|e| http::transport(e, "Twitch validation"))?;
        let data: Validation = http::json(
            http::checked(response, "Twitch validation").await?,
            "Twitch validation",
        )
        .await?;
        if data.client_id != self.client_id || data.expires_in <= 60 {
            self.token = None;
            return Err(http::protocol(
                "Twitch validation",
                "jeton expiré ou destiné à une autre application",
            ));
        }
        let now = Instant::now();
        let expires_at = now
            .checked_add(Duration::from_secs(data.expires_in))
            .ok_or_else(|| http::protocol("Twitch validation", "durée de validité invalide"))?;
        let token = self.token.as_mut().expect("token acquired");
        token.validated_at = Some(now);
        token.expires_at = expires_at;
        Ok(())
    }

    async fn ensure_token(&mut self) -> Result<()> {
        let refresh = self.token.as_ref().is_none_or(|t| {
            t.expires_at.saturating_duration_since(Instant::now()) <= Duration::from_secs(60)
        });
        if refresh {
            self.acquire_token().await?;
        }
        let validate = refresh
            || self.token.as_ref().is_some_and(|t| {
                t.validated_at
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(3600))
            });
        if !validate {
            return Ok(());
        }
        let Err(error) = self.validate_token().await else {
            return Ok(());
        };
        self.token = None;
        // A revoked application token can be replaced without user OAuth.
        if error
            .downcast_ref::<http::ApiError>()
            .is_some_and(|e| e.status == Some(StatusCode::UNAUTHORIZED))
        {
            self.acquire_token().await?;
            self.validate_token().await?;
            Ok(())
        } else {
            Err(error)
        }
    }
    async fn get<T: DeserializeOwned>(
        &mut self,
        endpoint: &str,
        query: &[(String, String)],
    ) -> Result<T> {
        for attempt in 0..2 {
            self.ensure_token().await?;
            let response = self
                .client
                .get(format!("{}/{endpoint}", self.api))
                .header("Client-Id", &self.client_id)
                .bearer_auth(&self.token.as_ref().expect("token acquired").value)
                .query(query)
                .send()
                .await
                .map_err(|e| http::transport(e, "Twitch API"))?;
            if response.status() == StatusCode::UNAUTHORIZED && attempt == 0 {
                self.token = None;
                continue;
            }
            return http::json(http::checked(response, "Twitch API").await?, "Twitch API").await;
        }
        unreachable!()
    }

    pub async fn french_bloodborne_streams(&mut self) -> Result<Vec<Stream>> {
        let game_id = match &self.game_id {
            Some(id) => id.clone(),
            None => {
                let games: Page<Game> = self
                    .get("games", &[("name".into(), "Bloodborne".into())])
                    .await?;
                let game = games
                    .data
                    .into_iter()
                    .find(|g| g.name.eq_ignore_ascii_case("Bloodborne") && !g.id.is_empty())
                    .ok_or_else(|| {
                        http::protocol("Twitch API", "catégorie Bloodborne introuvable")
                    })?;
                self.game_id = Some(game.id.clone());
                game.id
            }
        };
        let mut streams = Vec::new();
        let mut cursor = None;
        let mut seen_cursors = HashSet::new();
        let mut seen_streams = HashSet::new();
        loop {
            let mut query = vec![
                ("game_id".into(), game_id.clone()),
                ("language".into(), "fr".into()),
                ("type".into(), "live".into()),
                ("first".into(), "100".into()),
            ];
            if let Some(cursor) = cursor {
                query.push(("after".into(), cursor));
            }
            let page: Page<Stream> = self.get("streams", &query).await?;
            for stream in page.data {
                if stream.matches(&game_id) {
                    stream.validate()?;
                    if seen_streams.insert(stream.id.clone()) {
                        streams.push(stream);
                    }
                }
            }
            cursor = page.pagination.cursor.filter(|s| !s.is_empty());
            let Some(ref next) = cursor else {
                break;
            };
            if !seen_cursors.insert(next.clone()) || seen_cursors.len() > 1000 {
                return Err(http::protocol(
                    "Twitch API",
                    "pagination incohérente, aucun état partiel enregistré",
                ));
            }
        }
        streams.sort_by(|a, b| a.started_at.cmp(&b.started_at).then(a.id.cmp(&b.id)));
        Ok(streams)
    }
}
