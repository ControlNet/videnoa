use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, RemoteJobId, TaskId, TaskStatus};
use crate::lifecycle::{LifecycleFailure, LifecycleService, RemoteAmbiguityStage};
use crate::persistence::{AttemptRecord, TaskRecord, WorkerRecord};
use crate::remote::{JobStatus, VidenoaClientError};

use super::{
    AssignedWorker, Reconciler, RecoveryCommandKind, RecoveryError, RecoveryReport, StagePermit,
};

impl Reconciler {
    /// Keeps a submission whose worker, not the request, failed: acceptance
    /// is unknown, so the attempt is confirmed with the same key later.
    pub(super) async fn defer_submission(
        &self,
        attempt: &AttemptRecord,
        worker: &WorkerRecord,
        error: VidenoaClientError,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        self.retry_submission(attempt, now, stage, &error).await?;
        self.defer_remote(worker, attempt.attempt.task_id, error, now, stage, report)
            .await
    }

    /// Fails a submission the worker answered for this request. Worker-level
    /// failures are deferred by the caller and never reach this.
    pub(super) async fn resolve_submission_failure(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        error: VidenoaClientError,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        let message = error.to_string();
        let task_failure = match error {
            VidenoaClientError::NotFound | VidenoaClientError::ClientStatus { .. } => {
                LifecycleFailure::terminal(
                    TaskStatus::Submitting,
                    FailureStage::Submission,
                    FailureCode::RemoteSubmissionFailed,
                    message,
                )
            }
            VidenoaClientError::Conflict
            | VidenoaClientError::MalformedPayload
            | VidenoaClientError::OversizedPayload { .. } => {
                LifecycleFailure::remote_state_ambiguous(RemoteAmbiguityStage::Submission, message)
            }
            VidenoaClientError::ServerStatus { .. }
            | VidenoaClientError::Network
            | VidenoaClientError::Timeout
            | VidenoaClientError::Stall
            | VidenoaClientError::RateLimited
            | VidenoaClientError::UnexpectedStatus { .. }
            | VidenoaClientError::Incompatible
            | VidenoaClientError::LocalIo
            | VidenoaClientError::InvalidFilePath
            | VidenoaClientError::EndpointUrl => return Err(error.into()),
        };
        let _write = stage.begin_write();
        LifecycleService::new(self.store.clone())
            .fail(task, Some(attempt), task_failure, now)
            .await?;
        report.push(task.id, RecoveryCommandKind::Terminal);
        Ok(())
    }

    /// A failed poll is no evidence about the job, so it never fails the
    /// task. `NotFound` is decided by the caller, after the worker passed its
    /// health check.
    pub(super) async fn resolve_processing_failure(
        &self,
        task: &TaskRecord,
        worker: &AssignedWorker,
        error: VidenoaClientError,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        if error == VidenoaClientError::MalformedPayload {
            // For example a job status added by a newer worker: the job is
            // still there, so keep treating it as running.
            tracing::warn!(task_id = %task.id, worker_id = %worker.record.id, "Remote job status is unreadable; polling continues");
            report.push(task.id, RecoveryCommandKind::Poll);
            return Ok(());
        }
        self.defer_remote(&worker.record, task.id, error, now, stage, report)
            .await
    }
}

/// Best-effort removal of a remote job that a task stopped tracking while it
/// may still run, so its GPU work is not orphaned. The task outcome is
/// already durable; a failure is only logged.
pub(super) async fn cancel_abandoned_job(
    worker: &AssignedWorker,
    task_id: TaskId,
    remote_job_id: RemoteJobId,
    status: JobStatus,
) {
    // An unknown status may be an active one.
    if !matches!(
        status,
        JobStatus::Queued | JobStatus::Running | JobStatus::Unknown
    ) {
        return;
    }
    match worker.client.cancel_job(remote_job_id).await {
        Ok(()) | Err(VidenoaClientError::NotFound) => {
            tracing::info!(%task_id, worker_id = %worker.record.id, %remote_job_id, "Abandoned remote job cancelled");
        }
        Err(error) => {
            tracing::warn!(%task_id, worker_id = %worker.record.id, %remote_job_id, %error, "Abandoned remote job could not be cancelled");
        }
    }
}
