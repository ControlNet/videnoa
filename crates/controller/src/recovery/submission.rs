use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, RemoteJobId, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, DurableAction, LifecycleErrorCode, LifecycleFailure, LifecycleService,
    SubmissionCancellationReconciliation, SubmissionEvidence,
};
use crate::persistence::{AttemptRecord, TaskRecord};
use crate::remote::{FileApiPath, JobStatus, VidenoaClientError};

use super::paths::{remote_paths, submission_params};
use super::submission_ownership::SubmissionOwnership;
use super::{
    AssignedWorker, Reconciler, RecoveryCommandKind, RecoveryError, RecoveryReport, StagePermit,
};

impl Reconciler {
    pub(super) async fn reconcile_submission(
        &self,
        mut task: TaskRecord,
        mut attempt: AttemptRecord,
        worker: &AssignedWorker,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        let service = LifecycleService::new(self.store.clone());
        let scheduler = crate::scheduler::Scheduler::load(self.store.clone())?;
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
        let submitted = match worker
            .client
            .run(
                &task.request.workflow,
                attempt.attempt.submission_key,
                &submission_params(input, output),
            )
            .await
        {
            Ok(submitted) => submitted,
            Err(error) if error.is_worker_level() => {
                return self
                    .defer_submission(&attempt, &worker.record, error, now, stage, report)
                    .await;
            }
            Err(error) => {
                return self
                    .resolve_submission_failure(&task, &attempt, error, now, stage, report)
                    .await;
            }
        };
        let write = stage.begin_write();
        let persisted = service
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
            .await;
        drop(write);
        if let Err(error) = persisted {
            if error.code() == LifecycleErrorCode::Conflict {
                self.retry_submission(&attempt, now, stage, &error).await?;
                report.defer(task.id);
                return Ok(());
            }
            return Err(error.into());
        }
        drop(admission);
        report.push(task.id, RecoveryCommandKind::Poll);
        Ok(())
    }

    pub(super) async fn reconcile_cancellation(
        &self,
        task: TaskRecord,
        attempt: AttemptRecord,
        worker: &AssignedWorker,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        match task.status {
            TaskStatus::Submitting => {
                self.cancel_submission(task, attempt, worker, now, stage, report)
                    .await
            }
            TaskStatus::Processing => {
                let remote_job_id = attempt
                    .attempt
                    .remote_job_id
                    .ok_or(RecoveryError::MissingRemoteEvidence)?;
                match stop_remote_job(worker, remote_job_id).await {
                    Ok(true) => {}
                    // Its files stay until the job stops using them.
                    Ok(false) => {
                        report.defer(task.id);
                        return Ok(());
                    }
                    Err(error) => {
                        // The cancel stays requested; the next pass retries it.
                        return self
                            .defer_remote(&worker.record, task.id, error, now, stage, report)
                            .await;
                    }
                }
                self.finish_cancellation(task, attempt, worker, now, stage, report)
                    .await
            }
            TaskStatus::Reserved
            | TaskStatus::Uploading
            | TaskStatus::Staged
            | TaskStatus::RemoteCompleted
            | TaskStatus::Downloading
            | TaskStatus::Verifying => {
                self.finish_cancellation(task, attempt, worker, now, stage, report)
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
        worker: &AssignedWorker,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        if self.claim_submission(&mut attempt, now).await? == SubmissionOwnership::Owned {
            report.defer(task.id);
            return Ok(());
        }
        let (input, output) = remote_paths(&attempt)?;
        // A protocol-2 Worker says whether the submission was accepted
        // without creating a job. An older one is asked by resubmitting
        // with the same key, which creates the job if it was not.
        let lookup = if worker.protocol >= 2 {
            worker
                .client
                .job_by_submission_key(attempt.attempt.submission_key)
                .await
                .map_err(|error| {
                    // Only the lookup's NotFound proves no job exists; a
                    // rejected lookup says nothing about the submission.
                    if let VidenoaClientError::ClientStatus { status, .. } = error {
                        if !error.is_worker_level() {
                            return VidenoaClientError::UnexpectedStatus { status };
                        }
                    }
                    error
                })
        } else {
            worker
                .client
                .run(
                    &task.request.workflow,
                    attempt.attempt.submission_key,
                    &submission_params(input, output),
                )
                .await
                .map(|submitted| submitted.receipt)
        };
        let reconciliation = match lookup {
            Ok(receipt) => SubmissionCancellationReconciliation::Accepted(SubmissionEvidence {
                remote_job_id: receipt.id,
                remote_input_path: input.clone(),
                remote_output_path: output.clone(),
            }),
            Err(error) if error.is_transient() || error == VidenoaClientError::RateLimited => {
                self.retry_submission(&attempt, now, stage, &error).await?;
                report.defer(task.id);
                return Ok(());
            }
            // Rejected credentials, or any other fault of the worker rather
            // than the request, do not mean the job was never accepted.
            Err(error) if error.is_worker_level() => {
                return self
                    .defer_submission(&attempt, &worker.record, error, now, stage, report)
                    .await;
            }
            Err(VidenoaClientError::NotFound | VidenoaClientError::ClientStatus { .. }) => {
                SubmissionCancellationReconciliation::NotAccepted
            }
            Err(error) => {
                return self
                    .defer_submission(&attempt, &worker.record, error, now, stage, report)
                    .await;
            }
        };
        let remote_job_id = match &reconciliation {
            SubmissionCancellationReconciliation::Accepted(value) => Some(value.remote_job_id),
            SubmissionCancellationReconciliation::NotAccepted => None,
        };
        let service = LifecycleService::new(self.store.clone());
        let write = stage.begin_write();
        let persisted = service
            .reconcile_submission_cancellation(&task, &attempt, reconciliation, now)
            .await;
        drop(write);
        if let Err(error) = persisted {
            if error.code() == LifecycleErrorCode::Conflict {
                self.retry_submission(&attempt, now, stage, &error).await?;
                report.defer(task.id);
                return Ok(());
            }
            return Err(error.into());
        }
        if let Some(job_id) = remote_job_id {
            match stop_remote_job(worker, job_id).await {
                Ok(true) => {}
                // The confirmed job is durable now; the next pass waits for
                // it to stop from Processing.
                Ok(false) => {
                    report.defer(task.id);
                    return Ok(());
                }
                Err(error) => {
                    // The confirmed job is durable now; the next pass cancels
                    // it from Processing.
                    return self
                        .defer_remote(&worker.record, task.id, error, now, stage, report)
                        .await;
                }
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
        self.finish_cancellation(task, attempt, worker, now, stage, report)
            .await
    }

    /// Deletes the task's workspace on the worker and marks it cancelled.
    /// Callers stop the task's remote job first: the job reads its input
    /// from the workspace until it stops.
    async fn finish_cancellation(
        &self,
        task: TaskRecord,
        attempt: AttemptRecord,
        worker: &AssignedWorker,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        crate::scheduler::remove_task_workspace(&self.config.paths, task.id)
            .await
            .map_err(RecoveryError::LocalCleanup)?;
        let workspace = FileApiPath::parse(&task.id.to_string())?;
        match worker.client.delete_file(&workspace).await {
            Ok(()) | Err(VidenoaClientError::NotFound) => {}
            Err(
                error @ (VidenoaClientError::LocalIo
                | VidenoaClientError::InvalidFilePath
                | VidenoaClientError::EndpointUrl),
            ) => return Err(error.into()),
            // The worker, not the cleanup, failed: cancellation retries later.
            Err(error) if error.is_worker_level() => {
                return self
                    .defer_remote(&worker.record, task.id, error, now, stage, report)
                    .await;
            }
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

/// Cancels a remote job and reports whether the worker has stopped it. A
/// worker keeps an active job listed, with its cancel requested, until its
/// executor exits; deleting a stopped job removes its history.
async fn stop_remote_job(
    worker: &AssignedWorker,
    id: RemoteJobId,
) -> Result<bool, VidenoaClientError> {
    match worker.client.cancel_job(id).await {
        Ok(()) | Err(VidenoaClientError::NotFound) => {}
        Err(error) => return Err(error),
    }
    match worker.client.job(id).await {
        Err(VidenoaClientError::NotFound) => Ok(true),
        Ok(job) => match job.status {
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled => {
                match worker.client.cancel_job(id).await {
                    Ok(()) | Err(VidenoaClientError::NotFound) => Ok(true),
                    Err(error) => Err(error),
                }
            }
            JobStatus::Queued | JobStatus::Running | JobStatus::Unknown => Ok(false),
        },
        Err(error) => Err(error),
    }
}
