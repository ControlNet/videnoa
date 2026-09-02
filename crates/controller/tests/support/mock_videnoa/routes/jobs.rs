use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use super::{body_bytes, error_response, journal_request, json_response, record, MAX_JSON_BYTES};
use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::journal::{JournalOutcome, Route};
use crate::mock_videnoa::state::SharedState;

mod submission;
pub(crate) use submission::run;

pub(crate) async fn poll(
    State(state): State<Arc<SharedState>>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let Ok((parts, body)) = body_bytes(request, MAX_JSON_BYTES).await else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_body");
    };
    let (sequence, job) = {
        let mut inner = state.inner.lock().await;
        let sequence = inner.begin(Route::JobPoll);
        (sequence, inner.persistent.jobs.get(&id).cloned())
    };
    let mut checkpoints = BTreeMap::new();
    state
        .checkpoint(Checkpoint::BeforePollResponse, &mut checkpoints)
        .await;
    let (status, response) = match job {
        Some(job) => (StatusCode::OK, json_response(StatusCode::OK, &job.response)),
        None => (
            StatusCode::NOT_FOUND,
            error_response(StatusCode::NOT_FOUND, "not_found"),
        ),
    };
    let journal = journal_request(&parts, &body, Route::JobPoll, sequence, checkpoints);
    record(&state, journal, status, JournalOutcome::Delivered).await;
    response
}

pub(crate) async fn cancel(
    State(state): State<Arc<SharedState>>,
    Path(id): Path<String>,
    request: Request,
) -> Response {
    let Ok((parts, body)) = body_bytes(request, MAX_JSON_BYTES).await else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_body");
    };
    let (sequence, found) = {
        let mut inner = state.inner.lock().await;
        let sequence = inner.begin(Route::JobCancel);
        let found = inner.persistent.jobs.remove(&id).is_some();
        if found {
            inner
                .persistent
                .idempotency
                .retain(|_, mapping| mapping.job_id != id);
            if state.persist_locked(&inner).await.is_err() {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, "persistence_failed");
            }
        }
        (sequence, found)
    };
    let status = if found {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    };
    let journal = journal_request(&parts, &body, Route::JobCancel, sequence, BTreeMap::new());
    record(&state, journal, status, JournalOutcome::Delivered).await;
    if found {
        status.into_response()
    } else {
        error_response(status, "not_found")
    }
}
