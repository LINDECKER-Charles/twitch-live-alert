use crate::{
    analytics::{Analytics, AnalyticsQuery},
    credentials::Credentials,
    notification::DesktopNotifier,
    rules::Rule,
    runtime::{ConnectionState, Control, PreferencesUpdate, Status},
    store::History,
    twitch::Category,
};
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, Request, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(test)]
const ADDRESS: &str = "127.0.0.1:37219";
#[cfg(test)]
const LOOPBACK_ALIAS: &str = "localhost:37219";
const RESPONSE_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: https://static-cdn.jtvnw.net; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'";

pub(crate) fn router(control: Arc<Control>, port: u16) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/style.css", get(style))
        .route("/{asset}", get(asset_script))
        .route(
            "/mark.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("../web/mark.svg"),
                )
            }),
        )
        .route("/api/status", get(status))
        .route("/api/categories", get(categories))
        .route("/api/rules", get(rules).post(upsert_rule))
        .route("/api/rules/{id}/delete", post(delete_rule))
        .route("/api/connection", get(connection))
        .route("/api/connection/verify", post(verify))
        .route("/api/connection/replace", post(replace))
        .route("/api/connection/disconnect", post(disconnect))
        .merge(control.mail.router())
        .route("/api/history", get(history))
        .route("/api/analytics", get(analytics))
        .route("/api/credentials", post(credentials))
        .route("/api/settings", post(settings))
        .route("/api/monitoring", post(monitoring))
        .route("/api/check-now", post(check_now))
        .route("/api/test-notification", post(test_notification))
        .route("/api/shutdown", post(shutdown))
        .with_state(control)
        .layer(DefaultBodyLimit::max(32 * 1024))
        .layer(middleware::from_fn_with_state(port, protect_local))
}

async fn protect_local(State(port): State<u16>, request: Request, next: Next) -> Response {
    let mut response = match validate_request(&request, port) {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(RESPONSE_POLICY),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn validate_request(request: &Request, port: u16) -> Result<(), ApiError> {
    let headers = request.headers();
    let host = single_header(headers, header::HOST.as_str())
        .filter(|host| *host == format!("127.0.0.1:{port}") || *host == format!("localhost:{port}"))
        .ok_or(ApiError(StatusCode::FORBIDDEN, "Adresse locale requise."))?;
    if headers
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
        && request.uri().path().starts_with("/api/")
    {
        return Err(ApiError(StatusCode::FORBIDDEN, "Requête locale requise."));
    }
    if request.method() != Method::POST {
        return Ok(());
    }
    let origin = format!("http://{host}");
    if single_header(headers, header::ORIGIN.as_str()) != Some(origin.as_str())
        || single_header(headers, "x-twitch-live-alert") != Some("local")
    {
        return Err(ApiError(StatusCode::FORBIDDEN, "Requête locale requise."));
    }
    let content_type = single_header(headers, header::CONTENT_TYPE.as_str())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !content_type.is_some_and(|value| value.eq_ignore_ascii_case("application/json")) {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Format JSON requis.",
        ));
    }
    Ok(())
}

fn single_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let all = headers.get_all(name);
    let mut values = all.iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

async fn index() -> Html<&'static str> {
    Html(include_str!("../web/index.html"))
}

async fn style() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../web/style.css"),
    )
}

async fn asset_script(Path(name): Path<String>) -> Response {
    match embedded_scripts(&name) {
        Some(source) => script(source).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

include!(concat!(env!("OUT_DIR"), "/scripts.rs"));

fn script(source: &'static str) -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        source,
    )
}

async fn status(State(control): State<Arc<Control>>) -> Json<Status> {
    Json(control.status())
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Search {
    q: String,
}

#[derive(Serialize)]
struct Categories {
    items: Vec<Category>,
}

fn query<T>(input: Result<Query<T>, QueryRejection>) -> Result<T, ApiError> {
    input.map(|Query(value)| value).map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "Paramètres de recherche invalides.",
        )
    })
}

async fn categories(
    State(control): State<Arc<Control>>,
    input: Result<Query<Search>, QueryRejection>,
) -> Result<Json<Categories>, ApiError> {
    Ok(Json(Categories {
        items: control.categories(query(input)?.q).await?,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    offset: u32,
    #[serde(default = "page_size")]
    limit: u32,
}

fn page_size() -> u32 {
    50
}

async fn history(
    State(control): State<Arc<Control>>,
    input: Result<Query<HistoryQuery>, QueryRejection>,
) -> Result<Json<History>, ApiError> {
    let q = query(input)?;
    Ok(Json(control.history(q.q, q.offset, q.limit).await?))
}

async fn analytics(
    State(control): State<Arc<Control>>,
    input: Result<Query<AnalyticsQuery>, QueryRejection>,
) -> Result<Json<Analytics>, ApiError> {
    Ok(Json(control.analytics(query(input)?).await?))
}

async fn credentials(
    State(control): State<Arc<Control>>,
    body: Result<Json<Credentials>, JsonRejection>,
) -> Result<Json<Status>, ApiError> {
    control.credentials(read_body(body)?).await?;
    Ok(Json(control.status()))
}

#[derive(Serialize)]
struct Rules {
    rules: Vec<Rule>,
}

async fn rules(State(control): State<Arc<Control>>) -> Json<Rules> {
    Json(Rules {
        rules: control.display_rules(),
    })
}

async fn upsert_rule(
    State(control): State<Arc<Control>>,
    body: Result<Json<Rule>, JsonRejection>,
) -> Result<Json<Rules>, ApiError> {
    Ok(Json(Rules {
        rules: control.upsert_rule(read_body(body)?).await?,
    }))
}

async fn delete_rule(
    State(control): State<Arc<Control>>,
    Path(id): Path<String>,
    body: Result<Json<Empty>, JsonRejection>,
) -> Result<Json<Rules>, ApiError> {
    read_body(body)?;
    Ok(Json(Rules {
        rules: control.delete_rule(&id).await?,
    }))
}

async fn connection(State(control): State<Arc<Control>>) -> Json<ConnectionState> {
    Json(control.connection())
}

async fn verify(
    State(control): State<Arc<Control>>,
    body: Result<Json<Empty>, JsonRejection>,
) -> Result<Json<ConnectionState>, ApiError> {
    read_body(body)?;
    Ok(Json(control.verify(true).await?))
}

async fn replace(
    State(control): State<Arc<Control>>,
    body: Result<Json<Credentials>, JsonRejection>,
) -> Result<Json<ConnectionState>, ApiError> {
    control.credentials(read_body(body)?).await?;
    Ok(Json(control.connection()))
}

async fn disconnect(
    State(control): State<Arc<Control>>,
    body: Result<Json<Empty>, JsonRejection>,
) -> Result<Json<ConnectionState>, ApiError> {
    read_body(body)?;
    Ok(Json(control.disconnect().await?))
}

async fn settings(
    State(control): State<Arc<Control>>,
    body: Result<Json<PreferencesUpdate>, JsonRejection>,
) -> Result<Json<Status>, ApiError> {
    control.change_preferences(read_body(body)?).await?;
    Ok(Json(control.status()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Monitoring {
    enabled: bool,
}

async fn monitoring(
    State(control): State<Arc<Control>>,
    body: Result<Json<Monitoring>, JsonRejection>,
) -> Result<Json<Status>, ApiError> {
    control.change_monitoring(read_body(body)?.enabled).await?;
    Ok(Json(control.status()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

async fn check_now(
    State(control): State<Arc<Control>>,
    body: Result<Json<Empty>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    read_body(body)?;
    control.check_now()?;
    Ok(StatusCode::ACCEPTED)
}

async fn test_notification(
    body: Result<Json<Empty>, JsonRejection>,
) -> Result<Json<Message>, ApiError> {
    read_body(body)?;
    let sent = tokio::task::spawn_blocking(|| DesktopNotifier::new()?.test_notification()).await;
    if !matches!(sent, Ok(Ok(()))) {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Windows n'a pas accepté la notification de test. Vérifiez les autorisations de notification.",
        ));
    }
    Ok(Json(Message {
        message: "Notification de test transmise à Windows.",
    }))
}

async fn shutdown(
    State(control): State<Arc<Control>>,
    body: Result<Json<Empty>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    read_body(body)?;
    control.stop();
    Ok(StatusCode::ACCEPTED)
}

fn read_body<T>(result: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    result
        .map(|Json(body)| body)
        .map_err(|error| ApiError(error.status(), "Corps JSON invalide ou trop volumineux."))
}

struct ApiError(StatusCode, &'static str);

impl From<crate::runtime::ControlError> for ApiError {
    fn from(error: crate::runtime::ControlError) -> Self {
        use crate::runtime::ControlError;
        match error {
            ControlError::Invalid(message) => Self(StatusCode::BAD_REQUEST, message),
            ControlError::Unavailable(message) => Self(StatusCode::CONFLICT, message),
            ControlError::Operation(message) => Self(StatusCode::INTERNAL_SERVER_ERROR, message),
        }
    }
}

#[derive(Serialize)]
struct Message {
    message: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(Message { message: self.1 })).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn local_requests_need_loopback_and_write_headers() {
        let router = Router::new()
            .route(
                "/",
                post(|| async { StatusCode::OK }).get(|| async { StatusCode::OK }),
            )
            .layer(middleware::from_fn_with_state(37219u16, protect_local));
        for (method, host, origin, marker, content_type, expected) in [
            (Method::GET, ADDRESS, "", "", "", StatusCode::OK),
            (
                Method::GET,
                "lookalike.example:37219",
                "",
                "",
                "",
                StatusCode::FORBIDDEN,
            ),
            (
                Method::POST,
                ADDRESS,
                "http://127.0.0.1:37219",
                "local",
                "application/json",
                StatusCode::OK,
            ),
            (
                Method::POST,
                LOOPBACK_ALIAS,
                "http://localhost:37219",
                "local",
                "application/json; charset=utf-8",
                StatusCode::OK,
            ),
            (
                Method::POST,
                ADDRESS,
                "",
                "local",
                "application/json",
                StatusCode::FORBIDDEN,
            ),
            (
                Method::POST,
                ADDRESS,
                "https://attacker.example",
                "local",
                "application/json",
                StatusCode::FORBIDDEN,
            ),
            (
                Method::POST,
                ADDRESS,
                "http://localhost:37219",
                "local",
                "application/json",
                StatusCode::FORBIDDEN,
            ),
            (
                Method::POST,
                ADDRESS,
                "http://127.0.0.1:37219",
                "",
                "application/json",
                StatusCode::FORBIDDEN,
            ),
            (
                Method::POST,
                ADDRESS,
                "http://127.0.0.1:37219",
                "local",
                "text/plain",
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ),
        ] {
            let mut request = axum::http::Request::builder()
                .method(method)
                .uri("/")
                .header("host", host);
            for (name, value) in [
                ("origin", origin),
                ("x-twitch-live-alert", marker),
                ("content-type", content_type),
            ] {
                if !value.is_empty() {
                    request = request.header(name, value);
                }
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            assert_eq!(
                response.headers()[header::CONTENT_SECURITY_POLICY],
                RESPONSE_POLICY
            );
            assert!(
                !response
                    .headers()
                    .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            );
        }
    }
}
