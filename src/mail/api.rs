use super::{
    MailService,
    settings::{Settings, Stored, Update},
    template::{self, Rendered, Templates},
};
use axum::{
    Json, Router,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize)]
struct View {
    #[serde(flatten)]
    settings: Settings,
    credentials_configured: bool,
    variables: &'static [&'static str],
    default_subject_template: &'static str,
    default_body_template: &'static str,
}

impl From<Stored> for View {
    fn from(stored: Stored) -> Self {
        Self {
            credentials_configured: stored.configured_auth(),
            settings: stored.settings,
            variables: template::VARIABLES,
            default_subject_template: template::DEFAULT_SUBJECT,
            default_body_template: template::DEFAULT_BODY,
        }
    }
}

struct Error(StatusCode, String);
impl Error {
    fn bad(error: impl ToString) -> Self {
        Self(StatusCode::BAD_REQUEST, error.to_string())
    }
    fn settings() -> Self {
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Lecture ou écriture des réglages SMTP impossible.".into(),
        )
    }
}

#[derive(Serialize)]
struct Message {
    message: String,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(Message { message: self.1 })).into_response()
    }
}

fn body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, Error> {
    body.map(|v| v.0)
        .map_err(|e| Error(e.status(), "Corps JSON invalide ou trop volumineux.".into()))
}

async fn load(path: PathBuf) -> Result<Stored, Error> {
    tokio::task::spawn_blocking(move || Stored::load(&path))
        .await
        .map_err(|_| Error::settings())?
        .map_err(|_| Error::settings())
}

async fn settings(State(service): State<MailService>) -> Result<Json<View>, Error> {
    Ok(Json(load(service.file()).await?.into()))
}

async fn update(
    State(service): State<MailService>,
    request: Result<Json<Update>, JsonRejection>,
) -> Result<Json<View>, Error> {
    let values = body(request)?;
    let path = service.file();
    let _guard = service.changes.lock().await;
    tokio::task::spawn_blocking(move || {
        let mut stored = Stored::load(&path).map_err(|_| Error::settings())?;
        stored.apply(values).map_err(Error::bad)?;
        stored.save(&path).map_err(|_| Error::settings())?;
        Ok(Json(stored.into()))
    })
    .await
    .map_err(|_| Error::settings())?
}

async fn preview(request: Result<Json<Templates>, JsonRejection>) -> Result<Json<Rendered>, Error> {
    let draft = body(request)?;
    Ok(Json(
        template::render(
            &draft.subject_template,
            &draft.body_template,
            &template::Context::sample(),
        )
        .map_err(Error::bad)?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

async fn test(
    State(service): State<MailService>,
    request: Result<Json<Empty>, JsonRejection>,
) -> Result<Json<Message>, Error> {
    body(request)?;
    let stored = load(service.file()).await?;
    stored.settings.validate().map_err(Error::bad)?;
    let mut content = template::render(
        &stored.settings.subject_template,
        &stored.settings.body_template,
        &template::Context::sample(),
    )
    .map_err(Error::bad)?;
    content.subject = format!("[Test] {}", content.subject);
    service
        .send(&stored, None, content)
        .await
        .map_err(|e| Error(StatusCode::SERVICE_UNAVAILABLE, e.to_string()))?;
    Ok(Json(Message {
        message: "Le serveur SMTP a accepté le courrier de test.".into(),
    }))
}

impl MailService {
    pub fn router<S: Clone + Send + Sync + 'static>(&self) -> Router<S> {
        Router::new()
            .route("/api/mail", get(settings).post(update))
            .route("/api/mail/delivery", get(super::delivery::get))
            .route("/api/mail/preview", post(preview))
            .route("/api/mail/test", post(test))
            .with_state(self.clone())
    }
}
