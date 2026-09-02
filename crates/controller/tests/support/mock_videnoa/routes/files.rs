use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::{stream, StreamExt};

use super::{
    body_bytes, error_response, invalid_remote_path_response, journal_request, json_response,
    record, MAX_FILE_BYTES, MAX_JSON_BYTES,
};
use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::domain::{FileStatResponse, UploadResponse};
use crate::mock_videnoa::faults::DeleteOutcome;
use crate::mock_videnoa::journal::{JournalOutcome, Route};
use crate::mock_videnoa::state::SharedState;

pub(crate) async fn upload(
    State(state): State<Arc<SharedState>>,
    Path(path): Path<String>,
    request: Request,
) -> Response {
    if let Some(response) = invalid_remote_path_response(&path) {
        return response;
    }
    let mut checkpoints = BTreeMap::new();
    state
        .checkpoint(Checkpoint::BeforeAcceptingUpload, &mut checkpoints)
        .await;
    let Ok((parts, body)) = body_bytes(request, MAX_FILE_BYTES).await else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_body");
    };
    let sequence = state.inner.lock().await.begin(Route::Upload);
    state
        .checkpoint(Checkpoint::AfterUploadBytesAccepted, &mut checkpoints)
        .await;
    let Ok(size) = u64::try_from(body.len()) else {
        return error_response(StatusCode::BAD_REQUEST, "file_too_large");
    };
    {
        let mut inner = state.inner.lock().await;
        inner.persistent.files.insert(path.clone(), body.to_vec());
        if state.persist_locked(&inner).await.is_err() {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "persistence_failed");
        }
    }
    let response_body = UploadResponse {
        path: format!("../mock-worker/workspace/{path}"),
        size,
    };
    let response = json_response(StatusCode::OK, &response_body);
    let journal = journal_request(&parts, &body, Route::Upload, sequence, checkpoints);
    record(&state, journal, StatusCode::OK, JournalOutcome::Delivered).await;
    response
}

pub(crate) async fn get(
    State(state): State<Arc<SharedState>>,
    Path(path): Path<String>,
    request: Request,
) -> Response {
    let Ok((parts, body)) = body_bytes(request, MAX_JSON_BYTES).await else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_body");
    };
    if let Some(file_path) = path.strip_suffix("/stat") {
        return stat(state, parts, body, file_path).await;
    }
    if let Some(response) = invalid_remote_path_response(&path) {
        return response;
    }
    download(state, parts, body, path).await
}

async fn stat(
    state: Arc<SharedState>,
    parts: axum::http::request::Parts,
    body: axum::body::Bytes,
    path: &str,
) -> Response {
    if let Some(response) = invalid_remote_path_response(path) {
        return response;
    }
    let (sequence, metadata) = {
        let mut inner = state.inner.lock().await;
        let sequence = inner.begin(Route::Stat);
        let file = inner.persistent.files.get(path);
        let is_dir = inner
            .persistent
            .files
            .keys()
            .any(|candidate| candidate.starts_with(&format!("{path}/")));
        let metadata = file
            .map(|bytes| (bytes.len(), true, false))
            .or_else(|| is_dir.then_some((0, false, true)));
        (sequence, metadata)
    };
    let (status, response) = match metadata {
        Some((size, is_file, is_dir)) => {
            let Ok(size) = u64::try_from(size) else {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, "size_overflow");
            };
            let metadata = FileStatResponse {
                path: format!("../mock-worker/workspace/{path}"),
                size,
                is_file,
                is_dir,
            };
            (StatusCode::OK, json_response(StatusCode::OK, &metadata))
        }
        None => (
            StatusCode::NOT_FOUND,
            error_response(StatusCode::NOT_FOUND, "not_found"),
        ),
    };
    let journal = journal_request(&parts, &body, Route::Stat, sequence, BTreeMap::new());
    record(&state, journal, status, JournalOutcome::Delivered).await;
    response
}

async fn download(
    state: Arc<SharedState>,
    parts: axum::http::request::Parts,
    body: axum::body::Bytes,
    path: String,
) -> Response {
    let mut checkpoints = BTreeMap::new();
    let (sequence, stored, truncate, corrupt, stall) = {
        let mut inner = state.inner.lock().await;
        let sequence = inner.begin(Route::Download);
        let stored = inner.persistent.files.get(&path).cloned();
        let truncate = inner.faults.truncate_download.take();
        let corrupt = inner.faults.corrupt_output.take();
        let stall = inner.faults.stall_download.take().is_some();
        (sequence, stored, truncate, corrupt, stall)
    };
    state
        .checkpoint(Checkpoint::BeforeDownloadBody, &mut checkpoints)
        .await;
    let Some(stored) = stored else {
        let journal = journal_request(&parts, &body, Route::Download, sequence, checkpoints);
        record(
            &state,
            journal,
            StatusCode::NOT_FOUND,
            JournalOutcome::Delivered,
        )
        .await;
        return error_response(StatusCode::NOT_FOUND, "not_found");
    };
    let (response_body, advertised, outcome) = match (truncate, corrupt, stall) {
        (Some(delivered), _, _) => {
            let delivered = delivered.min(stored.len());
            let chunks = [
                Ok(Bytes::copy_from_slice(&stored[..delivered])),
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "injected truncated download",
                )),
            ];
            (
                Body::from_stream(stream::iter(chunks)),
                stored.len(),
                JournalOutcome::Truncated {
                    advertised_bytes: stored.len(),
                    delivered_bytes: delivered,
                },
            )
        }
        (None, Some(bytes), _) => {
            let len = bytes.len();
            (Body::from(bytes), len, JournalOutcome::CorruptOutput)
        }
        (None, None, true) => {
            let advertised = stored.len();
            let first = stored.into_iter().take(1).collect::<Vec<_>>();
            let chunks = stream::once(async move { Ok::<_, std::io::Error>(Bytes::from(first)) })
                .chain(stream::pending());
            (
                Body::from_stream(chunks),
                advertised,
                JournalOutcome::Delivered,
            )
        }
        (None, None, false) => {
            let len = stored.len();
            let chunks = stream::unfold((stored, 0_usize), |(bytes, offset)| async move {
                if offset >= bytes.len() {
                    return None;
                }
                let end = (offset + 8 * 1024).min(bytes.len());
                let chunk = Bytes::copy_from_slice(&bytes[offset..end]);
                Some((Ok::<_, std::io::Error>(chunk), (bytes, end)))
            });
            (Body::from_stream(chunks), len, JournalOutcome::Delivered)
        }
    };
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/octet-stream"),
        )
        .header(header::CONTENT_LENGTH, advertised.to_string())
        .body(response_body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    let journal = journal_request(&parts, &body, Route::Download, sequence, checkpoints);
    record(&state, journal, StatusCode::OK, outcome).await;
    response
}

pub(crate) async fn delete(
    State(state): State<Arc<SharedState>>,
    Path(path): Path<String>,
    request: Request,
) -> Response {
    if let Some(response) = invalid_remote_path_response(&path) {
        return response;
    }
    let Ok((parts, body)) = body_bytes(request, MAX_JSON_BYTES).await else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_body");
    };
    let mut checkpoints = BTreeMap::new();
    state
        .checkpoint(Checkpoint::BeforeDelete, &mut checkpoints)
        .await;
    let (sequence, status, outcome) = {
        let mut inner = state.inner.lock().await;
        let sequence = inner.begin(Route::DeleteFile);
        let scripted = inner.faults.delete_script.pop_front();
        let exists = inner.persistent.files.contains_key(&path)
            || inner
                .persistent
                .files
                .keys()
                .any(|candidate| candidate.starts_with(&format!("{path}/")));
        let result = scripted.unwrap_or(if exists {
            DeleteOutcome::Success
        } else {
            DeleteOutcome::NotFound
        });
        let (status, outcome) = match result {
            DeleteOutcome::NotFound => (StatusCode::NOT_FOUND, JournalOutcome::FaultStatus),
            DeleteOutcome::ServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                JournalOutcome::FaultStatus,
            ),
            DeleteOutcome::Success => {
                inner.persistent.files.retain(|candidate, _| {
                    candidate != &path && !candidate.starts_with(&format!("{path}/"))
                });
                if state.persist_locked(&inner).await.is_err() {
                    return error_response(StatusCode::INTERNAL_SERVER_ERROR, "persistence_failed");
                }
                (StatusCode::NO_CONTENT, JournalOutcome::Delivered)
            }
        };
        (sequence, status, outcome)
    };
    state
        .checkpoint(Checkpoint::AfterDelete, &mut checkpoints)
        .await;
    let response = if status == StatusCode::NO_CONTENT {
        status.into_response()
    } else {
        error_response(status, "delete_failed")
    };
    let journal = journal_request(&parts, &body, Route::DeleteFile, sequence, checkpoints);
    record(&state, journal, status, outcome).await;
    response
}
