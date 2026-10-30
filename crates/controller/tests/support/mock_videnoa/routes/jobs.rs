use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use super::{
    body_bytes, error_response, journal_request, json_response, raw_json_response, record,
    MAX_JSON_BYTES,
};
use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::domain::{
    is_active, mock_time, CancelReason, ErrorCode, ErrorResponse, JobRecord,
};
use crate::mock_videnoa::journal::{JournalOutcome, Route};
use crate::mock_videnoa::state::SharedState;

mod submission;
pub(crate) use submission::{run, validate};

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
    let (status, response) = match state.take_response_fault(Route::JobPoll).await {
        Some(fault) => match StatusCode::from_u16(fault.status) {
            Ok(status) => (status, raw_json_response(status, fault.body)),
            Err(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                error_response(StatusCode::INTERNAL_SERVER_ERROR, "invalid_fault_status"),
            ),
        },
        None => match job {
            Some(job) => (StatusCode::OK, json_response(StatusCode::OK, &job.response)),
            None => (
                StatusCode::NOT_FOUND,
                error_response(StatusCode::NOT_FOUND, "not_found"),
            ),
        },
    };
    let journal = journal_request(&parts, &body, Route::JobPoll, sequence, checkpoints);
    record(&state, journal, status, JournalOutcome::Delivered).await;
    response
}

/// `GET /api/jobs/by-idempotency-key/{key}`: the job a keyed run created.
/// A protocol-1 worker has no such route and answers a bare 404.
pub(crate) async fn lookup(
    State(state): State<Arc<SharedState>>,
    Path(key): Path<String>,
    request: Request,
) -> Response {
    let Ok((parts, body)) = body_bytes(request, MAX_JSON_BYTES).await else {
        return error_response(StatusCode::BAD_REQUEST, "invalid_body");
    };
    if state
        .protocol
        .lock()
        .await
        .is_none_or(|protocol| protocol < 2)
    {
        return error_response(StatusCode::NOT_FOUND, "not_found");
    }
    let (sequence, creation) = {
        let mut inner = state.inner.lock().await;
        let sequence = inner.begin(Route::JobLookup);
        let creation = inner
            .persistent
            .idempotency
            .get(&key)
            .and_then(|mapping| inner.persistent.jobs.get(&mapping.job_id))
            .map(JobRecord::creation_response);
        (sequence, creation)
    };
    let (status, response) = match state.take_response_fault(Route::JobLookup).await {
        Some(fault) => {
            let status =
                StatusCode::from_u16(fault.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, raw_json_response(status, fault.body))
        }
        None => match creation {
            Some(creation) => (StatusCode::OK, json_response(StatusCode::OK, &creation)),
            None => (
                StatusCode::NOT_FOUND,
                json_response(
                    StatusCode::NOT_FOUND,
                    &ErrorResponse {
                        error: "no job was created with this idempotency key".to_owned(),
                        code: Some(ErrorCode::JobNotFound),
                    },
                ),
            ),
        },
    };
    let journal = journal_request(&parts, &body, Route::JobLookup, sequence, BTreeMap::new());
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
    if let Some(fault) = state.take_response_fault(Route::JobCancel).await {
        let sequence = state.inner.lock().await.begin(Route::JobCancel);
        let status =
            StatusCode::from_u16(fault.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let journal = journal_request(&parts, &body, Route::JobCancel, sequence, BTreeMap::new());
        record(&state, journal, status, JournalOutcome::FaultStatus).await;
        return raw_json_response(status, fault.body);
    }
    let (sequence, found) = {
        let mut inner = state.inner.lock().await;
        let sequence = inner.begin(Route::JobCancel);
        let slow = inner.faults.slow_job_cancellation;
        let stopping = inner
            .persistent
            .jobs
            .get_mut(&id)
            .filter(|job| slow && is_active(job.response.status));
        if let Some(job) = stopping {
            // The executor is still winding down: only record the request.
            job.response.cancel_requested = true;
            job.response.cancel_requested_at = Some(mock_time("2026-09-02T00:00:03Z"));
            job.response.cancel_reason = Some(CancelReason::User);
            if state.persist_locked(&inner).await.is_err() {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, "persistence_failed");
            }
            drop(inner);
            let journal =
                journal_request(&parts, &body, Route::JobCancel, sequence, BTreeMap::new());
            record(
                &state,
                journal,
                StatusCode::NO_CONTENT,
                JournalOutcome::Delivered,
            )
            .await;
            return StatusCode::NO_CONTENT.into_response();
        }
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
