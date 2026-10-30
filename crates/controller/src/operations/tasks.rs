use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;

use crate::domain::{
    AbandonTaskResponse, CancelTaskResponse, DiscardTaskResponse, RemoteJobId, RetryTaskRequest,
    RetryTaskResponse, TaskActionRequest, TaskId, WorkerId, WorkflowName,
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

/// Gives up on a cancellation that waits for an unreachable Worker. Only the
/// Controller's own temporary files are released: the Worker is not contacted.
pub(super) async fn abandon(
    State(state): State<OperationsState>,
    id: Result<Path<TaskId>, PathRejection>,
    payload: Result<Json<TaskActionRequest>, JsonRejection>,
) -> Result<Json<AbandonTaskResponse>, OperationsError> {
    let Path(id) = id.map_err(|_| OperationsError::InvalidRequest)?;
    let Json(request) = payload.map_err(|_| OperationsError::InvalidRequest)?;
    let task = task(&state, id).await?;
    require_version(task.version, request.version)?;
    let attempt = state
        .store
        .current_attempt(id)
        .await
        .map_err(|_| OperationsError::Internal)?
        .ok_or(OperationsError::Conflict("task has no attempt to abandon"))?;
    let committed = state
        .lifecycle
        .abandon(&task, &attempt, Utc::now())
        .await
        .map_err(|error| OperationsError::from_lifecycle(&error))?;
    if let Err(error) = crate::scheduler::remove_task_workspace(&state.paths, id).await {
        tracing::warn!(task_id = %id, %error, "Abandoned task kept its temporary files");
    }
    Ok(Json(AbandonTaskResponse {
        task_id: id,
        status: committed.status(),
    }))
}

/// Gives up on a failed task: it can no longer be retried, and its files are
/// released. The Controller's temporary files always go; the task folder on
/// the Worker goes only when the Worker is online and the task's job, if any,
/// has ended. The published destination is never touched.
pub(super) async fn discard(
    State(state): State<OperationsState>,
    id: Result<Path<TaskId>, PathRejection>,
    payload: Result<Json<TaskActionRequest>, JsonRejection>,
) -> Result<Json<DiscardTaskResponse>, OperationsError> {
    let Path(id) = id.map_err(|_| OperationsError::InvalidRequest)?;
    let Json(request) = payload.map_err(|_| OperationsError::InvalidRequest)?;
    // A running retry deletes and recreates the Worker folder.
    let _in_flight = state.retries_in_flight.claim(id)?;
    let task = task(&state, id).await?;
    require_version(task.version, request.version)?;
    let attempt = state
        .store
        .current_attempt(id)
        .await
        .map_err(|_| OperationsError::Internal)?;
    let committed = state
        .lifecycle
        .discard(&task, Utc::now())
        .await
        .map_err(|error| OperationsError::from_lifecycle(&error))?;
    if let Err(error) = crate::scheduler::remove_task_workspace(&state.paths, id).await {
        tracing::warn!(task_id = %id, %error, "Discarded task kept its temporary files");
    }
    let worker_folder_deleted = match attempt {
        Some(attempt) => delete_worker_folder(&state, &task, &attempt).await,
        None => true,
    };
    Ok(Json(DiscardTaskResponse {
        task_id: id,
        status: committed.status(),
        worker_folder_deleted,
    }))
}

/// Deletes a discarded task's folder on the Worker of its attempt when that
/// is safe. Returns whether the folder is known to be gone.
async fn delete_worker_folder(
    state: &OperationsState,
    task: &crate::persistence::TaskRecord,
    attempt: &crate::persistence::AttemptRecord,
) -> bool {
    let Some(worker_id) = attempt.attempt.worker_id else {
        return false;
    };
    let worker = match state.workers.worker(worker_id).await {
        Ok(Some(worker)) if worker.online => worker,
        Ok(_) | Err(_) => return false,
    };
    let Ok(client) = VidenoaClient::new_with_password(
        worker.api_url,
        state.scheduler.runtime_settings().remote_timeouts(),
        state.payload_limits,
        worker.password.as_ref(),
    ) else {
        return false;
    };
    if let Some(remote_job_id) = attempt.attempt.remote_job_id {
        // A job that may still run reads and writes the folder.
        if terminal_job(&client, task, attempt, remote_job_id)
            .await
            .is_err()
        {
            return false;
        }
    }
    let Ok(workspace) = FileApiPath::parse(&task.id.to_string()) else {
        return false;
    };
    match client.delete_file(&workspace).await {
        Ok(()) | Err(VidenoaClientError::NotFound) => true,
        Err(error) => {
            tracing::warn!(task_id = %task.id, %error, "Discarded task kept its Worker folder");
            false
        }
    }
}

pub(super) async fn retry(
    State(state): State<OperationsState>,
    id: Result<Path<TaskId>, PathRejection>,
    payload: Result<Json<RetryTaskRequest>, JsonRejection>,
) -> Result<Json<RetryTaskResponse>, OperationsError> {
    let Path(id) = id.map_err(|_| OperationsError::InvalidRequest)?;
    let Json(request) = payload.map_err(|_| OperationsError::InvalidRequest)?;
    let _in_flight = state.retries_in_flight.claim(id)?;
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
    if task.discarded_at.is_some() {
        return Err(OperationsError::Conflict("task was discarded"));
    }
    if attempt.attempt.worker_id.is_none() {
        // Its Worker was deleted: no output, workspace or job can be checked.
        return Err(OperationsError::Conflict(
            "the Worker that ran this task was deleted",
        ));
    }
    let moved = request
        .worker_id
        .filter(|worker_id| Some(*worker_id) != attempt.attempt.worker_id);
    let (committed, attempt_id) = match (Lifecycle::retry_mode(failure), moved) {
        (mode @ RetryMode::NewProcessingAttempt, _)
        | (mode @ RetryMode::Resume(ResumeStage::Uploading | ResumeStage::Staged), Some(_)) => {
            let requested = request.worker_id;
            (
                requeue(&state, &task, &attempt, mode, requested).await?,
                None,
            )
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

/// Task ids whose retry is still running. A requeue deletes the remote
/// workspace before its CAS, so a second retry of the same version must not
/// start until the first has committed; it would otherwise delete the input
/// that the new attempt uploads.
#[derive(Clone, Default)]
pub(super) struct RetriesInFlight(
    std::sync::Arc<std::sync::Mutex<std::collections::HashSet<TaskId>>>,
);

impl RetriesInFlight {
    fn claim(&self, id: TaskId) -> Result<RetryClaim, OperationsError> {
        let mut claimed = self.0.lock().map_err(|_| OperationsError::Internal)?;
        if !claimed.insert(id) {
            return Err(OperationsError::Conflict(
                "task retry is already in progress",
            ));
        }
        Ok(RetryClaim {
            retries: self.clone(),
            id,
        })
    }
}

struct RetryClaim {
    retries: RetriesInFlight,
    id: TaskId,
}

impl Drop for RetryClaim {
    fn drop(&mut self) {
        if let Ok(mut claimed) = self.retries.0.lock() {
            claimed.remove(&self.id);
        }
    }
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
    let runnable = capabilities
        .workflows
        .iter()
        .any(|summary| &summary.name == workflow)
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
