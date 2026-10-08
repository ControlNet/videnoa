use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;

use crate::domain::{
    CancelTaskResponse, RemoteJobId, RetryTaskRequest, RetryTaskResponse, TaskActionRequest,
    TaskId, WorkerId, WorkflowName,
};
use crate::lifecycle::{
    Lifecycle, RemoteTerminalStatus, RequeueRetryCommand, ResumeStage, RetryMode,
    TerminalRemoteEvidence, WorkspaceCleaned,
};
use crate::remote::{FileApiPath, JobStatus, VidenoaClient, VidenoaClientError};

use super::request_failure::OperationsError;
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
    payload: Result<Json<RetryTaskRequest>, JsonRejection>,
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
    let moved = request
        .worker_id
        .filter(|worker_id| Some(*worker_id) != attempt.attempt.worker_id);
    let (committed, attempt_id) = match (Lifecycle::retry_mode(failure), moved) {
        (mode @ RetryMode::NewProcessingAttempt, _)
        | (mode @ RetryMode::Resume(ResumeStage::Uploading | ResumeStage::Staged), Some(_)) => {
            let requested = request.worker_id;
            (requeue(&state, &task, &attempt, mode, requested).await?, None)
        }
        (RetryMode::Resume(_) | RetryMode::Blocked, None) => (
            state
                .lifecycle
                .retry_downstream(&task, &attempt, Utc::now())
                .await
                .map_err(|error| OperationsError::from_lifecycle(&error))?,
            Some(attempt.attempt.id),
        ),
        (RetryMode::Resume(_) | RetryMode::Blocked, Some(_)) => {
            return Err(OperationsError::InvalidField(
                "worker_id",
                "This failure can only be retried on the Worker that holds its output",
            ));
        }
    };
    Ok(Json(RetryTaskResponse {
        task_id: id,
        attempt_id,
        status: committed.status(),
    }))
}

/// Deletes the task workspace on the failed attempt's Worker, after checking
/// that its remote job (if any) is terminal, then queues the task for
/// `requested` (default: the same Worker).
async fn requeue(
    state: &OperationsState,
    task: &crate::persistence::TaskRecord,
    attempt: &crate::persistence::AttemptRecord,
    mode: RetryMode,
    requested: Option<WorkerId>,
) -> Result<crate::lifecycle::CommittedCommand, OperationsError> {
    let original_id = attempt
        .attempt
        .worker_id
        .ok_or(OperationsError::RemoteStateAmbiguous)?;
    let requested = requested.unwrap_or(original_id);
    ensure_retry_worker(state, requested, &task.request.workflow).await?;
    let original = state
        .workers
        .worker(original_id)
        .await
        .map_err(|error| OperationsError::from_worker(&error))?
        .ok_or(OperationsError::RemoteStateAmbiguous)?;
    let client = VidenoaClient::new_with_password(
        original.api_url,
        state.scheduler.runtime_settings().remote_timeouts(),
        state.payload_limits,
        original.password.as_ref(),
    )
    .map_err(|_| OperationsError::Internal)?;
    let terminal = match (attempt.attempt.remote_job_id, mode) {
        (Some(remote_job_id), _) => {
            Some(terminal_job(&client, task, attempt, remote_job_id).await?)
        }
        (None, RetryMode::NewProcessingAttempt) => {
            return Err(OperationsError::RemoteStateAmbiguous);
        }
        (None, _) => None,
    };
    let workspace =
        FileApiPath::parse(&task.id.to_string()).map_err(|_| OperationsError::Internal)?;
    match client.delete_file(&workspace).await {
        Ok(()) | Err(VidenoaClientError::NotFound) => {}
        Err(error) => return Err(OperationsError::from_remote(&error)),
    }
    state
        .lifecycle
        .retry_requeue(
            task,
            attempt,
            &RequeueRetryCommand {
                requested_worker_id: requested,
                terminal,
                workspace: WorkspaceCleaned::new(task.id),
            },
            Utc::now(),
        )
        .await
        .map_err(|error| OperationsError::from_lifecycle(&error))
}

async fn terminal_job(
    client: &VidenoaClient,
    task: &crate::persistence::TaskRecord,
    attempt: &crate::persistence::AttemptRecord,
    remote_job_id: RemoteJobId,
) -> Result<TerminalRemoteEvidence, OperationsError> {
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
    Ok(TerminalRemoteEvidence::new(remote_job_id, terminal))
}

/// Rejects a retry Worker that is unknown, disabled, or does not report the workflow.
async fn ensure_retry_worker(
    state: &OperationsState,
    worker_id: WorkerId,
    workflow: &WorkflowName,
) -> Result<(), OperationsError> {
    let worker = state
        .workers
        .worker(worker_id)
        .await
        .map_err(|error| OperationsError::from_worker(&error))?
        .ok_or(OperationsError::InvalidField(
            "worker_id",
            "Worker was not found",
        ))?;
    if !worker.enabled {
        return Err(OperationsError::InvalidField(
            "worker_id",
            "Worker is disabled",
        ));
    }
    let capabilities = &worker.capabilities;
    let runnable = capabilities.workflows.iter().any(|summary| &summary.name == workflow)
        && !capabilities
            .invalid_workflows
            .iter()
            .any(|invalid| &invalid.name == workflow);
    if !runnable {
        return Err(OperationsError::InvalidField(
            "worker_id",
            "Worker does not report this task's workflow as runnable",
        ));
    }
    Ok(())
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
