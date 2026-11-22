use crate::http;
use anyhow::Result;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

const API: &str = "https://api.twitch.tv/helix";
const OAUTH: &str = "https://id.twitch.tv/oauth2";

#[derive(Clone, Debug, Deserialize, Serialize)]
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
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub game_name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub is_mature: Option<bool>,
    #[serde(default)]
    pub thumbnail_url: String,
}

impl Stream {
    pub fn matches(&self, game_id: &str, language: &str) -> bool {
        self.kind == "live"
            && self.game_id == game_id
            && (language.is_empty() || self.language.eq_ignore_ascii_case(language))
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
#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Category {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) box_art_url: String,
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

#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(default)]
pub struct StreamFilter {
    pub category: String,
    pub language: String,
}

impl Default for StreamFilter {
    fn default() -> Self {
        Self {
            category: "Bloodborne".into(),
            language: "fr".into(),
        }
    }
}

impl StreamFilter {
    pub(crate) fn valid(&self) -> bool {
        !self.category.trim().is_empty()
            && self.category.chars().count() <= 200
            && !self.category.chars().any(char::is_control)
            && (self.language.is_empty()
                || self.language == "other"
                || (self.language.len() == 2
                    && self.language.bytes().all(|b| b.is_ascii_lowercase())))
    }

    pub(crate) fn baseline_key(&self) -> String {
        format!(
            "initialized:{}:{}",
            self.language,
            self.category.to_lowercase()
        )
    }
}

#[derive(Clone)]
pub struct Twitch {
    session: Arc<tokio::sync::Mutex<Session>>,
}

struct Session {
    client: Client,
    client_id: String,
    client_secret: String,
    api: String,
    oauth: String,
    token: Option<Token>,
    cooldown: Option<Instant>,
    categories: HashMap<String, Category>,
    request_times: std::collections::VecDeque<Instant>,
}

impl Twitch {
    pub fn new(client: Client, client_id: String, client_secret: String) -> Self {
        let session = Session {
            client,
            client_id,
            client_secret,
            api: API.into(),
            oauth: OAUTH.into(),
            token: None,
            cooldown: None,
            categories: HashMap::new(),
            request_times: Default::default(),
        };
        Self {
            session: Arc::new(tokio::sync::Mutex::new(session)),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(client: Client, base: &str) -> Self {
        let twitch = Self::new(client, "test-client".into(), "test-secret".into());
        {
            let mut session = twitch.session.try_lock().unwrap();
            session.api = format!("{base}/helix");
            session.oauth = format!("{base}/oauth2");
        }
        twitch
    }

    pub(crate) fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.session, &other.session)
    }

    pub(crate) async fn verify(&self) -> Result<()> {
        let mut session = self.session.lock().await;
        if let Some(token) = session.token.as_mut() {
            token.validated_at = None;
        }
        session.ensure_token().await
    }

    pub async fn validate_credentials(&self) -> Result<()> {
        self.session.lock().await.ensure_token().await
    }
}

impl Session {
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
        if let Some(remaining) = self
            .cooldown
            .and_then(|until| until.checked_duration_since(Instant::now()))
        {
            return Err(http::ApiError {
                service: "Twitch API",
                status: Some(StatusCode::TOO_MANY_REQUESTS),
                retry_after: Some(remaining),
                reason: "limite de requêtes atteinte",
            }
            .into());
        }
        let result = self.request(endpoint, query).await;
        if let Err(error) = &result
            && let Some(api) = error.downcast_ref::<http::ApiError>()
            && api.status == Some(StatusCode::TOO_MANY_REQUESTS)
        {
            let delay = api.retry_after.unwrap_or(Duration::from_secs(60));
            if let Some(until) = Instant::now().checked_add(delay) {
                self.cooldown = self.cooldown.max(Some(until));
            }
        }
        result
    }

    async fn request<T: DeserializeOwned>(
        &mut self,
        endpoint: &str,
        query: &[(String, String)],
    ) -> Result<T> {
        for attempt in 0..2 {
            self.ensure_token().await?;
            // Share the rolling budget across rules and category search. The session serializes requests.
            const BUDGET: usize = 120;
            const WINDOW: Duration = Duration::from_secs(60);
            while self
                .request_times
                .front()
                .is_some_and(|t| t.elapsed() >= WINDOW)
            {
                self.request_times.pop_front();
            }
            if self.request_times.len() >= BUDGET {
                tokio::time::sleep(WINDOW.saturating_sub(self.request_times[0].elapsed())).await;
                self.request_times.pop_front();
            }
            self.request_times.push_back(Instant::now());
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
            if response
                .headers()
                .get("ratelimit-remaining")
                .and_then(|v| v.to_str().ok())
                == Some("0")
                && let Some(reset) = response
                    .headers()
                    .get("ratelimit-reset")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
            {
                self.cooldown = Instant::now().checked_add(Duration::from_secs(
                    reset.saturating_sub(http::unix_seconds()).saturating_add(1),
                ));
            }
            let response = http::checked(response, "Twitch API").await?;
            return http::json(response, "Twitch API").await;
        }
        unreachable!()
    }
}

impl Twitch {
    pub(crate) fn cached_categories(&self) -> HashMap<String, Category> {
        self.session
            .try_lock()
            .map(|s| s.categories.clone())
            .unwrap_or_default()
    }

    pub(crate) async fn category(&mut self, name: &str) -> Result<Category> {
        let mut session = self.session.lock().await;
        let key = name.to_lowercase();
        if let Some(category) = session.categories.get(&key) {
            return Ok(category.clone());
        }
        let page: Page<Category> = session
            .get("games", &[("name".into(), name.into())])
            .await?;
        let category = page
            .data
            .into_iter()
            .find(|category| !category.id.is_empty() && category.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| http::protocol("Twitch API", "catégorie introuvable"))?;
        if session.categories.len() >= 128 {
            session.categories.clear();
        }
        session.categories.insert(key, category.clone());
        Ok(category)
    }

    pub(crate) async fn search_categories(&self, query: &str) -> Result<Vec<Category>> {
        let page: Page<Category> = self
            .session
            .lock()
            .await
            .get(
                "search/categories",
                &[
                    ("query".into(), query.into()),
                    ("first".into(), "12".into()),
                ],
            )
            .await?;
        Ok(page
            .data
            .into_iter()
            .filter(|item| !item.id.is_empty() && !item.name.is_empty())
            .collect())
    }

    async fn stream_page(
        &self,
        query: &[(String, String)],
        continuation: bool,
    ) -> Result<Page<Stream>> {
        let page = self.session.lock().await.get("streams", query).await;
        let Err(error) = &page else { return page };
        let Some(api) = error.downcast_ref::<http::ApiError>() else {
            return page;
        };
        let delay = api.retry_after.unwrap_or(Duration::from_secs(60));
        // Keep a valid cursor across one ordinary token-bucket reset. Return persistent
        // failures to the scheduler without publishing an incomplete snapshot.
        if continuation
            && api.status == Some(StatusCode::TOO_MANY_REQUESTS)
            && delay <= Duration::from_secs(120)
        {
            tokio::time::sleep(delay).await;
            return self.session.lock().await.get("streams", query).await;
        }
        page
    }

    pub async fn streams(&mut self, filter: &StreamFilter) -> Result<Vec<Stream>> {
        let game_id = self.category(&filter.category).await?.id;
        let mut streams = Vec::new();
        let mut cursor = None;
        let mut seen_cursors = HashSet::new();
        let mut seen_streams = HashSet::new();
        loop {
            let mut query = vec![
                ("game_id".into(), game_id.clone()),
                ("type".into(), "live".into()),
                ("first".into(), "100".into()),
            ];
            if !filter.language.is_empty() {
                query.push(("language".into(), filter.language.clone()));
            }
            if let Some(cursor) = cursor {
                query.push(("after".into(), cursor));
            }
            let page = self.stream_page(&query, !seen_cursors.is_empty()).await?;
            for stream in page.data {
                if stream.matches(&game_id, &filter.language) {
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
