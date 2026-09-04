use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, RetryMetadata, TaskStatus};
use crate::persistence::{AttemptRecord, TaskRecord};

use super::engine::{applied, attempt_cas};
use super::{
    CommittedCommand, DurableAction, Lifecycle, LifecycleError, LifecycleService,
    ProcessingRetryCommand, ProcessingRetryWrite, ResumeStage, RetryMode, RetryWrite,
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

    /// Creates a new attempt only after terminal remote and workspace-cleanup evidence.
    ///
    /// # Errors
    /// Returns an error for blocked retry, mismatched evidence, reused identity, or CAS conflict.
    pub async fn retry_processing(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        command: &ProcessingRetryCommand,
        occurred_at: DateTime<Utc>,
    ) -> Result<CommittedCommand, LifecycleError> {
        let failure = task
            .failure
            .as_ref()
            .ok_or(LifecycleError::IllegalCommand)?;
        if Lifecycle::retry_mode(failure) != RetryMode::NewProcessingAttempt {
            return Err(retry_error(failure.failure_code));
        }
        let old_attempt = attempt_cas(task, attempt)?;
        let remote_job_id = attempt
            .attempt
            .remote_job_id
            .ok_or(LifecycleError::RemoteEvidenceMismatch)?;
        if command.terminal.job_id() != remote_job_id {
            return Err(LifecycleError::RemoteEvidenceMismatch);
        }
        match command.terminal.status() {
            super::RemoteTerminalStatus::Completed
            | super::RemoteTerminalStatus::Failed
            | super::RemoteTerminalStatus::Cancelled => {}
        }
        if command.workspace.task_id() != task.id
            || command.workspace.remote_job_id() != remote_job_id
        {
            return Err(LifecycleError::WorkspaceEvidenceMismatch);
        }
        if command.attempt_id == old_attempt.id
            || command.submission_key == attempt.attempt.submission_key
        {
            return Err(LifecycleError::Conflict);
        }
        let write = ProcessingRetryWrite {
            task_id: task.id,
            task_version: task.version,
            old_attempt,
            new_attempt_id: command.attempt_id,
            worker_id: command.worker_id,
            submission_key: command.submission_key,
            remote_job_id,
            occurred_at,
        };
        let version = applied(self.store().retry_processing_attempt(&write).await?)?;
        Ok(self.committed(task.id, TaskStatus::Reserved, version, DurableAction::None))
    }
}

const fn stage_action(stage: ResumeStage) -> DurableAction {
    match stage {
        ResumeStage::Uploading => DurableAction::Upload,
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
