use anyhow::Result;
use reqwest::{Client, Response, StatusCode};
use serde::de::DeserializeOwned;
use std::{
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub struct ApiError {
    pub service: &'static str,
    pub status: Option<StatusCode>,
    pub retry_after: Option<Duration>,
    pub reason: &'static str,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} : {}", self.service, self.reason)?;
        if let Some(status) = self.status {
            write!(f, " (HTTP {})", status.as_u16())?;
        }
        Ok(())
    }
}
impl std::error::Error for ApiError {}

pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(concat!(
            "alert-bloodborne-fr-live/",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

// Never propagate reqwest's error: its URL can contain a Discord webhook token.
pub fn transport(error: reqwest::Error, service: &'static str) -> anyhow::Error {
    ApiError {
        service,
        status: error.status(),
        retry_after: None,
        reason: if error.is_timeout() {
            "délai réseau dépassé"
        } else {
            "échec réseau"
        },
    }
    .into()
}

pub async fn checked(response: Response, service: &'static str) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let mut retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_delay);
    if status == StatusCode::TOO_MANY_REQUESTS && retry_after.is_none() {
        retry_after = response
            .headers()
            .get("ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(|reset| {
                Duration::from_secs(reset.saturating_sub(unix_seconds()).saturating_add(1))
            });
        if retry_after.is_none() {
            // Discord may send a fractional retry_after only in the JSON body.
            if let Ok(body) = response.json::<serde_json::Value>().await {
                retry_after = body
                    .get("retry_after")
                    .and_then(|v| v.as_f64())
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .and_then(|s| Duration::try_from_secs_f64(s).ok());
            }
        }
    }
    Err(ApiError {
        service,
        status: Some(status),
        retry_after,
        reason: "requête refusée",
    }
    .into())
}

fn parse_delay(value: &str) -> Option<Duration> {
    value
        .parse::<f64>()
        .ok()
        .filter(|s| s.is_finite() && *s >= 0.0)
        .and_then(|s| Duration::try_from_secs_f64(s).ok())
}

pub async fn json<T: DeserializeOwned>(response: Response, service: &'static str) -> Result<T> {
    response.json().await.map_err(|_| {
        ApiError {
            service,
            status: None,
            retry_after: None,
            reason: "réponse JSON invalide",
        }
        .into()
    })
}

pub fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn retry_delay(error: &anyhow::Error, failures: u32, poll: Duration) -> Duration {
    let fallback = Duration::from_secs(5 * 2u64.pow(failures.saturating_sub(1).min(6)));
    let requested = error
        .downcast_ref::<ApiError>()
        .and_then(|e| e.retry_after)
        .unwrap_or_default();
    // Never retry earlier than an API's explicit cooldown.
    poll.max(fallback).max(requested)
}

pub fn protocol(service: &'static str, reason: &'static str) -> anyhow::Error {
    ApiError {
        service,
        status: None,
        retry_after: None,
        reason,
    }
    .into()
}
