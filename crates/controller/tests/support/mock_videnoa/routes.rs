use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::{header, request::Parts, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Serialize;
use serde_json::json;

use super::journal::{snapshot_headers, JournalOutcome, JournalRequest, Route};
use super::state::SharedState;

mod catalog;
mod files;
mod jobs;

pub(crate) const DROP_RESPONSE_HEADER: &str = "x-mock-videnoa-drop-response";
pub(crate) const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_JSON_BYTES: usize = 1024 * 1024;

pub(crate) fn router(state: Arc<SharedState>) -> Router {
    Router::new()
        .route("/api/health", get(catalog::health))
        .route("/api/workflows", get(catalog::workflows))
        .route("/api/presets", get(catalog::presets))
        .route(
            "/api/workflows/{filename}/interface",
            get(catalog::workflow_interface),
        )
        .route("/api/run", post(jobs::run))
        .route("/api/jobs/{id}", get(jobs::poll).delete(jobs::cancel))
        .route(
            "/api/files/{*path}",
            get(files::get).put(files::upload).delete(files::delete),
        )
        .fallback(not_found)
        .with_state(state)
}

async fn not_found() -> Response {
    error_response(StatusCode::NOT_FOUND, "not_found")
}

pub(crate) async fn body_bytes(request: Request, limit: usize) -> Result<(Parts, Bytes), Response> {
    let (parts, body) = request.into_parts();
    axum::body::to_bytes(body, limit)
        .await
        .map(|bytes| (parts, bytes))
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "invalid_body"))
}

pub(crate) fn journal_request(
    parts: &Parts,
    body: &[u8],
    route: Route,
    sequence: u64,
    checkpoints: BTreeMap<String, super::journal::LogicalTimestamp>,
) -> JournalRequest {
    JournalRequest {
        sequence,
        method: parts.method.clone(),
        path: parts.uri.path().to_owned(),
        headers: snapshot_headers(&parts.headers),
        body: body.to_vec(),
        route,
        checkpoints,
    }
}

pub(crate) async fn record(
    state: &SharedState,
    request: JournalRequest,
    status: StatusCode,
    outcome: JournalOutcome,
) {
    state
        .inner
        .lock()
        .await
        .journal
        .push(request.finish(status.as_u16(), outcome));
}

pub(crate) fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(bytes) => Response::builder()
            .status(status)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )
            .body(Body::from(bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub(crate) fn error_response(status: StatusCode, code: &str) -> Response {
    json_response(status, &json!({"error": code}))
}

pub(crate) fn invalid_remote_path_response(path: &str) -> Option<Response> {
    let invalid = path.is_empty()
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."));
    if invalid {
        Some(error_response(StatusCode::BAD_REQUEST, "invalid_path"))
    } else {
        None
    }
}
