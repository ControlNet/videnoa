use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, DurableAction, LifecycleErrorCode, LifecycleFailure, LifecycleService,
    SubmissionCancellationReconciliation, SubmissionEvidence,
};
use crate::persistence::{AttemptRecord, TaskRecord};
use crate::remote::{FileApiPath, VidenoaClient, VidenoaClientError};

use super::paths::{remote_paths, submission_params};
use super::submission_ownership::SubmissionOwnership;
use super::{Reconciler, RecoveryCommandKind, RecoveryError, RecoveryReport, StagePermit};

impl Reconciler {
    pub(super) async fn reconcile_submission(
        &self,
        mut task: TaskRecord,
        mut attempt: AttemptRecord,
        client: &VidenoaClient,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        let service = LifecycleService::new(self.store.clone());
        let scheduler = crate::scheduler::Scheduler::load(self.store.clone()).await?;
        let mut admission = None;
        if task.status == TaskStatus::Staged {
            self.checkpoint(crate::scheduler::TransferCheckpointPoint::BeforeRemoteSubmit)
                .await;
            admission = scheduler.admit(DurableAction::Submit).await?;
            if admission.is_none() {
                report.defer(task.id);
                return Ok(());
            }
            let write = stage.begin_write();
            let transition = service
                .advance(&task, &attempt, AdvanceCommand::StartSubmission, now)
                .await;
            drop(write);
            if let Err(error) = transition {
                if error.code() == LifecycleErrorCode::Conflict {
                    report.defer(task.id);
                    return Ok(());
                }
                return Err(error.into());
            }
            task = self
                .store
                .task(task.id)
                .await?
                .ok_or(RecoveryError::Conflict)?;
            attempt = self
                .store
                .current_attempt(task.id)
                .await?
                .ok_or(RecoveryError::MissingAttempt)?;
        } else {
            self.checkpoint(crate::scheduler::TransferCheckpointPoint::BeforeRemoteSubmit)
                .await;
        }
        if self.claim_submission(&mut attempt, now).await? == SubmissionOwnership::Owned {
            report.defer(task.id);
            return Ok(());
        }
        let (input, output) = remote_paths(&attempt)?;
        let submitted = match client
            .run(
                &task.request.workflow,
                attempt.attempt.submission_key,
                &submission_params(input, output),
            )
            .await
        {
            Ok(submitted) => submitted,
            Err(error) => {
                return self
                    .resolve_submission_failure(&task, &attempt, error, now, stage, report)
                    .await;
            }
        };
        let _write = stage.begin_write();
        service
            .advance(
                &task,
                &attempt,
                AdvanceCommand::PersistSubmission(SubmissionEvidence {
                    remote_job_id: submitted.receipt.id,
                    remote_input_path: input.clone(),
                    remote_output_path: output.clone(),
                }),
                now,
            )
            .await?;
        drop(admission);
        report.push(task.id, RecoveryCommandKind::Poll);
        Ok(())
    }

    pub(super) async fn reconcile_cancellation(
        &self,
        task: TaskRecord,
        attempt: AttemptRecord,
        client: &VidenoaClient,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        match task.status {
            TaskStatus::Submitting => {
                self.cancel_submission(task, attempt, client, now, stage, report)
                    .await
            }
            TaskStatus::Processing => {
                let remote_job_id = attempt
                    .attempt
                    .remote_job_id
                    .ok_or(RecoveryError::MissingRemoteEvidence)?;
                match client.cancel_job(remote_job_id).await {
                    Ok(()) | Err(VidenoaClientError::NotFound) => {}
                    Err(error) => return Err(error.into()),
                }
                self.finish_cancellation(task, attempt, client, now, stage, report)
                    .await
            }
            TaskStatus::Reserved
            | TaskStatus::Uploading
            | TaskStatus::Staged
            | TaskStatus::RemoteCompleted
            | TaskStatus::Downloading
            | TaskStatus::Verifying => {
                self.finish_cancellation(task, attempt, client, now, stage, report)
                    .await
            }
            TaskStatus::Queued
            | TaskStatus::Publishing
            | TaskStatus::RemoteCleanup
            | TaskStatus::Completed
            | TaskStatus::Failed
            | TaskStatus::Cancelled => Err(RecoveryError::Conflict),
        }
    }

    async fn cancel_submission(
        &self,
        task: TaskRecord,
        mut attempt: AttemptRecord,
        client: &VidenoaClient,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        if self.claim_submission(&mut attempt, now).await? == SubmissionOwnership::Owned {
            report.defer(task.id);
            return Ok(());
        }
        let (input, output) = remote_paths(&attempt)?;
        let reconciliation = match client
            .run(
                &task.request.workflow,
                attempt.attempt.submission_key,
                &submission_params(input, output),
            )
            .await
        {
            Ok(submitted) => SubmissionCancellationReconciliation::Accepted(SubmissionEvidence {
                remote_job_id: submitted.receipt.id,
                remote_input_path: input.clone(),
                remote_output_path: output.clone(),
            }),
            Err(VidenoaClientError::NotFound | VidenoaClientError::ClientStatus { .. }) => {
                SubmissionCancellationReconciliation::NotAccepted
            }
            Err(error) => return Err(error.into()),
        };
        let remote_job_id = match &reconciliation {
            SubmissionCancellationReconciliation::Accepted(value) => Some(value.remote_job_id),
            SubmissionCancellationReconciliation::NotAccepted => None,
        };
        let service = LifecycleService::new(self.store.clone());
        let write = stage.begin_write();
        service
            .reconcile_submission_cancellation(&task, &attempt, reconciliation, now)
            .await?;
        drop(write);
        if let Some(job_id) = remote_job_id {
            match client.cancel_job(job_id).await {
                Ok(()) | Err(VidenoaClientError::NotFound) => {}
                Err(error) => return Err(error.into()),
            }
        }
        let task = self
            .store
            .task(task.id)
            .await?
            .ok_or(RecoveryError::Conflict)?;
        let attempt = self
            .store
            .current_attempt(task.id)
            .await?
            .ok_or(RecoveryError::MissingAttempt)?;
        self.finish_cancellation(task, attempt, client, now, stage, report)
            .await
    }

    async fn finish_cancellation(
        &self,
        task: TaskRecord,
        attempt: AttemptRecord,
        client: &VidenoaClient,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        crate::scheduler::remove_task_workspace(&self.config.paths, task.id)
            .await
            .map_err(RecoveryError::LocalCleanup)?;
        let workspace = FileApiPath::parse(&task.id.to_string())?;
        match client.delete_file(&workspace).await {
            Ok(()) | Err(VidenoaClientError::NotFound) => {}
            Err(
                error @ (VidenoaClientError::ServerStatus { .. }
                | VidenoaClientError::Network
                | VidenoaClientError::Timeout
                | VidenoaClientError::Stall
                | VidenoaClientError::LocalIo
                | VidenoaClientError::InvalidFilePath
                | VidenoaClientError::EndpointUrl),
            ) => return Err(error.into()),
            Err(error) => {
                let _write = stage.begin_write();
                LifecycleService::new(self.store.clone())
                    .fail_recovery(
                        &task,
                        Some(&attempt),
                        LifecycleFailure::terminal(
                            task.status,
                            FailureStage::RemoteCleanup,
                            FailureCode::CleanupFailed,
                            error.to_string(),
                        ),
                        now,
                    )
                    .await?;
                report.push(task.id, RecoveryCommandKind::Terminal);
                return Ok(());
            }
        }
        let _write = stage.begin_write();
        LifecycleService::new(self.store.clone())
            .finish_cancellation(&task, &attempt, now)
            .await?;
        report.push(task.id, RecoveryCommandKind::Terminal);
        Ok(())
    }
}
