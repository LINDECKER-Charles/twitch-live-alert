use crate::{
    notification::DesktopNotifier,
    runtime::{ADDRESS, Control, Preferences, Status},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const LOOPBACK_ALIAS: &str = "localhost:37219";
const RESPONSE_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'";

pub(crate) fn router(control: Arc<Control>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/style.css", get(style))
        .route("/app.js", get(script))
        .route("/api/status", get(status))
        .route("/api/settings", post(settings))
        .route("/api/monitoring", post(monitoring))
        .route("/api/check-now", post(check_now))
        .route("/api/test-notification", post(test_notification))
        .route("/api/shutdown", post(shutdown))
        .with_state(control)
        .layer(DefaultBodyLimit::max(4096))
        .layer(middleware::from_fn(protect_local))
}

async fn protect_local(request: Request, next: Next) -> Response {
    let mut response = match validate_request(&request) {
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

fn validate_request(request: &Request) -> Result<(), ApiError> {
    let headers = request.headers();
    let host = single_header(headers, header::HOST.as_str())
        .filter(|host| matches!(*host, ADDRESS | LOOPBACK_ALIAS))
        .ok_or(ApiError(StatusCode::FORBIDDEN, "Adresse locale requise."))?;
    if request.method() != Method::POST {
        return Ok(());
    }
    let origin = single_header(headers, header::ORIGIN.as_str());
    if origin
        != Some(if host == ADDRESS {
            "http://127.0.0.1:37219"
        } else {
            "http://localhost:37219"
        })
        || single_header(headers, "x-bloodborne-request") != Some("local")
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

async fn script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/app.js"),
    )
}

async fn status(State(control): State<Arc<Control>>) -> Json<Status> {
    Json(control.status())
}

async fn settings(
    State(control): State<Arc<Control>>,
    body: Result<Json<Preferences>, JsonRejection>,
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
            .layer(middleware::from_fn(protect_local));
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
                ("x-bloodborne-request", marker),
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
