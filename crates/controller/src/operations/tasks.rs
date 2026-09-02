use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;

use crate::domain::{
    AttemptId, CancelTaskResponse, RetryTaskResponse, SubmissionKey, TaskActionRequest, TaskId,
};
use crate::lifecycle::{
    Lifecycle, ProcessingRetryCommand, RemoteTerminalStatus, RetryMode, TerminalRemoteEvidence,
    WorkspaceCleaned,
};
use crate::remote::{FileApiPath, JobStatus, VidenoaClient, VidenoaClientError};

use super::error::OperationsError;
use super::OperationsState;

pub(super) async fn cancel(
    State(state): State<OperationsState>,
    id: Result<Path<TaskId>, PathRejection>,
    payload: Result<Json<TaskActionRequest>, JsonRejection>,
) -> Result<Json<CancelTaskResponse>, OperationsError> {
    let Path(id) = id.map_err(|_| OperationsError::InvalidRequest)?;
    let Json(request) = payload.map_err(|_| OperationsError::InvalidRequest)?;
    let task = task(&state, id).await?;
    require_version(task.version, request.version)?;
    let attempt = state
        .store
        .current_attempt(id)
        .await
        .map_err(|_| OperationsError::Internal)?;
    let requested_at = Utc::now();
    let committed = state
        .lifecycle
        .request_cancellation(&task, attempt.as_ref(), requested_at)
        .await
        .map_err(|error| OperationsError::from_lifecycle(&error))?;
    Ok(Json(CancelTaskResponse {
        task_id: id,
        status: committed.status(),
        cancel_requested_at: requested_at,
    }))
}

pub(super) async fn retry(
    State(state): State<OperationsState>,
    id: Result<Path<TaskId>, PathRejection>,
    payload: Result<Json<TaskActionRequest>, JsonRejection>,
) -> Result<Json<RetryTaskResponse>, OperationsError> {
    let Path(id) = id.map_err(|_| OperationsError::InvalidRequest)?;
    let Json(request) = payload.map_err(|_| OperationsError::InvalidRequest)?;
    let task = task(&state, id).await?;
    require_version(task.version, request.version)?;
    let attempt = state
        .store
        .current_attempt(id)
        .await
        .map_err(|_| OperationsError::Internal)?
        .ok_or(OperationsError::Conflict("task has no retryable attempt"))?;
    let failure = task
        .failure
        .as_ref()
        .ok_or(OperationsError::Conflict("task has no retryable failure"))?;
    let (committed, attempt_id) = match Lifecycle::retry_mode(failure) {
        RetryMode::Resume(_) | RetryMode::Blocked => (
            state
                .lifecycle
                .retry_downstream(&task, &attempt, Utc::now())
                .await
                .map_err(|error| OperationsError::from_lifecycle(&error))?,
            attempt.attempt.id,
        ),
        RetryMode::NewProcessingAttempt => processing_retry(&state, &task, &attempt).await?,
    };
    Ok(Json(RetryTaskResponse {
        task_id: id,
        attempt_id,
        status: committed.status(),
    }))
}

async fn processing_retry(
    state: &OperationsState,
    task: &crate::persistence::TaskRecord,
    attempt: &crate::persistence::AttemptRecord,
) -> Result<(crate::lifecycle::CommittedCommand, AttemptId), OperationsError> {
    let worker_id = attempt
        .attempt
        .worker_id
        .ok_or(OperationsError::RemoteStateAmbiguous)?;
    let remote_job_id = attempt
        .attempt
        .remote_job_id
        .ok_or(OperationsError::RemoteStateAmbiguous)?;
    let worker = state
        .workers
        .worker(worker_id)
        .await
        .map_err(|error| OperationsError::from_worker(&error))?
        .ok_or(OperationsError::RemoteStateAmbiguous)?;
    let client = VidenoaClient::new(
        worker.api_url,
        state.scheduler.runtime_settings().remote_timeouts(),
        state.payload_limits,
    )
    .map_err(|_| OperationsError::Internal)?;
    let job = client
        .job(remote_job_id)
        .await
        .map_err(|error| OperationsError::from_remote(&error))?;
    if !crate::recovery::remote_job_identity_matches(task, attempt, &job) {
        return Err(OperationsError::RemoteStateAmbiguous);
    }
    let terminal = match job.status {
        JobStatus::Completed => RemoteTerminalStatus::Completed,
        JobStatus::Failed => RemoteTerminalStatus::Failed,
        JobStatus::Cancelled => RemoteTerminalStatus::Cancelled,
        JobStatus::Queued | JobStatus::Running => {
            return Err(OperationsError::Conflict(
                "remote processing is not terminal",
            ));
        }
    };
    let workspace =
        FileApiPath::parse(&task.id.to_string()).map_err(|_| OperationsError::Internal)?;
    match client.delete_file(&workspace).await {
        Ok(()) | Err(VidenoaClientError::NotFound) => {}
        Err(error) => return Err(OperationsError::from_remote(&error)),
    }
    let attempt_id = AttemptId::random();
    let committed = state
        .lifecycle
        .retry_processing(
            task,
            attempt,
            &ProcessingRetryCommand {
                attempt_id,
                worker_id,
                submission_key: SubmissionKey::random(),
                terminal: TerminalRemoteEvidence::new(remote_job_id, terminal),
                workspace: WorkspaceCleaned::new(task.id, remote_job_id),
            },
            Utc::now(),
        )
        .await
        .map_err(|error| OperationsError::from_lifecycle(&error))?;
    Ok((committed, attempt_id))
}

async fn task(
    state: &OperationsState,
    id: TaskId,
) -> Result<crate::persistence::TaskRecord, OperationsError> {
    state
        .store
        .task(id)
        .await
        .map_err(|_| OperationsError::Internal)?
        .ok_or(OperationsError::NotFound("task was not found"))
}

fn require_version(actual: u64, expected: u64) -> Result<(), OperationsError> {
    if actual == expected {
        Ok(())
    } else {
        Err(OperationsError::Conflict("task changed since it was read"))
    }
}
