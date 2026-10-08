use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, RetryMetadata, TaskStatus};
use crate::persistence::{AttemptRecord, TaskRecord};

use super::engine::{applied, attempt_cas};
use super::{
    CommittedCommand, DurableAction, Lifecycle, LifecycleError, LifecycleService,
    RequeueRetryCommand, RequeueRetryWrite, ResumeStage, RetryMode, RetryWrite,
    TransferRetryWrite,
};

impl LifecycleService {
    pub(crate) async fn schedule_transfer_retry(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        retry: RetryMetadata,
        occurred_at: DateTime<Utc>,
    ) -> Result<CommittedCommand, LifecycleError> {
        let write = TransferRetryWrite {
            task_id: task.id,
            task_version: task.version,
            attempt: attempt_cas(task, attempt)?,
            retry,
            occurred_at,
        };
        let version = applied(self.store().schedule_transfer_retry(&write).await?)?;
        Ok(self.committed(task.id, task.status, version, DurableAction::None))
    }

    /// Resumes a failed stage on the existing attempt without repeating compute.
    ///
    /// # Errors
    /// Returns an error for blocked failures, inconsistent history, or stale CAS state.
    pub async fn retry_downstream(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        occurred_at: DateTime<Utc>,
    ) -> Result<CommittedCommand, LifecycleError> {
        let failure = task
            .failure
            .as_ref()
            .ok_or(LifecycleError::IllegalCommand)?;
        let RetryMode::Resume(stage) = Lifecycle::retry_mode(failure) else {
            return Err(retry_error(failure.failure_code));
        };
        let write = RetryWrite {
            task_id: task.id,
            task_version: task.version,
            attempt: attempt_cas(task, attempt)?,
            target: stage.status(),
            occurred_at,
        };
        let version = applied(self.store().retry_lifecycle_stage(&write).await?)?;
        Ok(self.committed(task.id, stage.status(), version, stage_action(stage)))
    }

    /// Returns a failed task to the queue for one Worker after its workspace on
    /// the failed attempt's Worker was deleted.
    ///
    /// Only failures before any output exists can be requeued: processing, which
    /// also needs terminal evidence for the remote job, and upload or rejected
    /// submission, which never created one. The failed attempt stays in history;
    /// the next reservation creates a new attempt.
    ///
    /// # Errors
    /// Returns an error for other failures, mismatched evidence, or CAS conflict.
    pub async fn retry_requeue(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        command: &RequeueRetryCommand,
        occurred_at: DateTime<Utc>,
    ) -> Result<CommittedCommand, LifecycleError> {
        let failure = task
            .failure
            .as_ref()
            .ok_or(LifecycleError::IllegalCommand)?;
        let remote_job_id = attempt.attempt.remote_job_id;
        match Lifecycle::retry_mode(failure) {
            RetryMode::NewProcessingAttempt => {
                let terminal = command
                    .terminal
                    .ok_or(LifecycleError::RemoteEvidenceMismatch)?;
                if Some(terminal.job_id()) != remote_job_id {
                    return Err(LifecycleError::RemoteEvidenceMismatch);
                }
                match terminal.status() {
                    super::RemoteTerminalStatus::Completed
                    | super::RemoteTerminalStatus::Failed
                    | super::RemoteTerminalStatus::Cancelled => {}
                }
            }
            RetryMode::Resume(ResumeStage::Uploading | ResumeStage::Staged) => {
                if remote_job_id.is_some() || command.terminal.is_some() {
                    return Err(LifecycleError::RemoteEvidenceMismatch);
                }
            }
            RetryMode::Resume(_) | RetryMode::Blocked => {
                return Err(retry_error(failure.failure_code));
            }
        }
        if command.workspace.task_id() != task.id {
            return Err(LifecycleError::WorkspaceEvidenceMismatch);
        }
        let write = RequeueRetryWrite {
            task_id: task.id,
            task_version: task.version,
            attempt: attempt_cas(task, attempt)?,
            requested_worker_id: command.requested_worker_id,
            occurred_at,
        };
        let version = applied(self.store().requeue_failed_task(&write).await?)?;
        Ok(self.committed(task.id, TaskStatus::Queued, version, DurableAction::None))
    }
}

const fn stage_action(stage: ResumeStage) -> DurableAction {
    match stage {
        ResumeStage::Uploading => DurableAction::Upload,
        ResumeStage::Staged => DurableAction::Submit,
        ResumeStage::Downloading => DurableAction::Download,
        ResumeStage::Verifying => DurableAction::Verify,
        ResumeStage::Publishing => DurableAction::Publish,
        ResumeStage::RemoteCleanup => DurableAction::Cleanup,
    }
}

const fn retry_error(code: FailureCode) -> LifecycleError {
    match code {
        FailureCode::RemoteStateAmbiguous => LifecycleError::RemoteStateAmbiguous,
        FailureCode::PublicationAmbiguous => LifecycleError::PublicationAmbiguous,
        FailureCode::InputUnavailable
        | FailureCode::InputChanged
        | FailureCode::OutputExists
        | FailureCode::WorkerUnavailable
        | FailureCode::WorkflowIncompatible
        | FailureCode::TransferFailed
        | FailureCode::RemoteSubmissionFailed
        | FailureCode::ProcessingFailed
        | FailureCode::VerificationFailed
        | FailureCode::PublicationFailed
        | FailureCode::CleanupFailed
        | FailureCode::Cancelled => LifecycleError::IllegalCommand,
    }
}
