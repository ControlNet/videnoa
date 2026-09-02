use chrono::{DateTime, Utc};

use crate::domain::TaskStatus;
use crate::lifecycle::{
    AdvanceCommand, LifecycleService, SubmissionCancellationReconciliation, SubmissionEvidence,
    UploadEvidence,
};
use crate::persistence::{AttemptRecord, TaskRecord};
use crate::remote::{sibling_output_path, VidenoaClient, VidenoaClientError};

use super::paths::{input_path, submission_params};
use super::{Reconciler, RecoveryCommandKind, RecoveryError, RecoveryReport, StagePermit};

impl Reconciler {
    pub(super) async fn reconcile_upload(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        client: &VidenoaClient,
        now: DateTime<Utc>,
        stage: &StagePermit,
    ) -> Result<(), RecoveryError> {
        let path = input_path(task)?;
        if let Ok(stat) = client.stat(&path).await {
            if !stat.is_file || stat.size != task.input_size {
                return Ok(());
            }
            let _write = stage.begin_write();
            let output = sibling_output_path(
                &stat.path,
                &format!("output.{}", task.output_extension.as_str()),
            )?;
            LifecycleService::new(self.store.clone())
                .advance(
                    task,
                    attempt,
                    AdvanceCommand::FinishUpload(UploadEvidence {
                        remote_input_path: stat.path,
                        remote_output_path: output,
                    }),
                    now,
                )
                .await?;
        }
        Ok(())
    }

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
        if task.status == TaskStatus::Staged {
            let write = stage.begin_write();
            service
                .advance(&task, &attempt, AdvanceCommand::StartSubmission, now)
                .await?;
            drop(write);
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
        }
        let (input, output) = remote_paths(&attempt)?;
        let submitted = client
            .run(
                &task.request.workflow,
                attempt.attempt.submission_key,
                &submission_params(input, output),
            )
            .await?;
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
                self.finish_cancellation(task, attempt, now, stage, report)
                    .await
            }
            TaskStatus::Reserved
            | TaskStatus::Uploading
            | TaskStatus::Staged
            | TaskStatus::RemoteCompleted
            | TaskStatus::Downloading
            | TaskStatus::Verifying => {
                self.finish_cancellation(task, attempt, now, stage, report)
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
        attempt: AttemptRecord,
        client: &VidenoaClient,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
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
        self.finish_cancellation(task, attempt, now, stage, report)
            .await
    }

    async fn finish_cancellation(
        &self,
        task: TaskRecord,
        attempt: AttemptRecord,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        let _write = stage.begin_write();
        LifecycleService::new(self.store.clone())
            .finish_cancellation(&task, &attempt, now)
            .await?;
        report.push(task.id, RecoveryCommandKind::Terminal);
        Ok(())
    }
}

fn remote_paths(
    attempt: &AttemptRecord,
) -> Result<(&crate::domain::RemotePath, &crate::domain::RemotePath), RecoveryError> {
    let input = attempt
        .attempt
        .remote_input_path
        .as_ref()
        .ok_or(RecoveryError::MissingRemoteEvidence)?;
    let output = attempt
        .attempt
        .remote_output_path
        .as_ref()
        .ok_or(RecoveryError::MissingRemoteEvidence)?;
    Ok((input, output))
}
