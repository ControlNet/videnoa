use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::lifecycle::{LifecycleFailure, LifecycleService, RemoteAmbiguityStage};
use crate::persistence::{AttemptRecord, TaskRecord};
use crate::remote::{CancelReason, Job, JobStatus, VidenoaClientError};
use crate::scheduler::TransferCheckpointPoint;

use super::remote_failure::cancel_abandoned_job;
use super::{
    AssignedWorker, Reconciler, RecoveryCommandKind, RecoveryError, RecoveryReport, StagePermit,
};

impl Reconciler {
    pub(super) async fn reconcile_processing(
        &self,
        mut task: TaskRecord,
        mut attempt: AttemptRecord,
        worker: &AssignedWorker,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        let Some(remote_job_id) = attempt.attempt.remote_job_id else {
            return self
                .fail_ambiguous(
                    &task,
                    Some(&attempt),
                    "durable attempt is missing remote submission evidence",
                    now,
                    stage,
                    report,
                )
                .await;
        };
        let service = LifecycleService::new(self.store.clone());
        match worker.client.job(remote_job_id).await {
            Ok(job) if !remote_job_identity_matches(&task, &attempt, &job) => {
                self.fail_ambiguous(
                    &task,
                    Some(&attempt),
                    "remote job identity contradicts durable submission evidence",
                    now,
                    stage,
                    report,
                )
                .await?;
                // Job IDs are random per submission: this attempt's job is abandoned.
                cancel_abandoned_job(worker, task.id, remote_job_id, job.status).await;
            }
            Ok(job) => {
                self.refresh_processing_progress(&mut task, &mut attempt, &job, now, stage)
                    .await?;
                match job.status {
                    JobStatus::Queued | JobStatus::Running => {
                        report.push(task.id, RecoveryCommandKind::Poll);
                    }
                    JobStatus::Unknown => {
                        // A status added by a newer worker: the job is still
                        // there, so keep treating it as running.
                        tracing::warn!(task_id = %task.id, worker_id = %worker.record.id, "Remote job status is unknown; polling continues");
                        report.push(task.id, RecoveryCommandKind::Poll);
                    }
                    JobStatus::Completed => {
                        let _write = stage.begin_write();
                        service
                            .advance(
                                &task,
                                &attempt,
                                crate::lifecycle::AdvanceCommand::FinishProcessing,
                                now,
                            )
                            .await?;
                        self.checkpoint(TransferCheckpointPoint::RemoteCompletionPersisted)
                            .await;
                        report.push(task.id, RecoveryCommandKind::Download);
                    }
                    JobStatus::Failed | JobStatus::Cancelled => {
                        let failure = if job.status == JobStatus::Failed {
                            LifecycleFailure::processing(
                                job.error
                                    .unwrap_or_else(|| "remote processing failed".to_owned()),
                            )
                        } else {
                            cancelled_failure(job.cancel_reason)
                        };
                        let _write = stage.begin_write();
                        service.fail(&task, Some(&attempt), failure, now).await?;
                        report.push(task.id, RecoveryCommandKind::Terminal);
                    }
                }
            }
            Err(VidenoaClientError::NotFound) => {
                let _write = stage.begin_write();
                service
                    .fail(
                        &task,
                        Some(&attempt),
                        LifecycleFailure::remote_state_ambiguous(
                            RemoteAmbiguityStage::Processing,
                            "durable remote job is missing from the worker",
                        ),
                        now,
                    )
                    .await?;
                report.push(task.id, RecoveryCommandKind::Terminal);
            }
            Err(error) => {
                self.resolve_processing_failure(&task, worker, error, now, stage, report)
                    .await?;
            }
        }
        Ok(())
    }
}

/// Why a job this Controller did not cancel ended cancelled, as the Worker
/// reports it (protocol 2). Without a reason it is the one cause older
/// Workers had: a restart.
fn cancelled_failure(reason: Option<CancelReason>) -> LifecycleFailure {
    match reason {
        Some(CancelReason::User) => {
            LifecycleFailure::processing("remote job was cancelled on the worker")
        }
        Some(CancelReason::Shutdown) => LifecycleFailure::restart_cancelled(
            "remote job was cancelled when the worker shut down",
        ),
        Some(CancelReason::Restart | CancelReason::Unknown) | None => {
            LifecycleFailure::restart_cancelled("remote job was cancelled during worker restart")
        }
    }
}

pub(crate) fn remote_job_identity_matches(
    task: &TaskRecord,
    attempt: &AttemptRecord,
    job: &Job,
) -> bool {
    let Some(remote_job_id) = attempt.attempt.remote_job_id else {
        return false;
    };
    let (Some(input), Some(output), Some(params)) = (
        attempt.attempt.remote_input_path.as_ref(),
        attempt.attempt.remote_output_path.as_ref(),
        job.params.as_ref(),
    ) else {
        return false;
    };
    job.id == remote_job_id
        && job.workflow_name.as_str() == task.request.workflow.run_name()
        && params.get("input") == Some(&Value::String(input.as_str().to_owned()))
        && params.get("output") == Some(&Value::String(output.as_str().to_owned()))
}
