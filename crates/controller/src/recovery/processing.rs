use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::lifecycle::{LifecycleFailure, LifecycleService, RemoteAmbiguityStage};
use crate::persistence::{AttemptRecord, TaskRecord};
use crate::remote::{Job, JobStatus, VidenoaClient, VidenoaClientError};

use super::{Reconciler, RecoveryCommandKind, RecoveryError, RecoveryReport, StagePermit};

impl Reconciler {
    pub(super) async fn reconcile_processing(
        &self,
        task: TaskRecord,
        attempt: AttemptRecord,
        client: &VidenoaClient,
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
        match client.job(remote_job_id).await {
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
            }
            Ok(job) => match job.status {
                JobStatus::Queued | JobStatus::Running => {
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
                    report.push(task.id, RecoveryCommandKind::Download);
                }
                JobStatus::Failed => {
                    let _write = stage.begin_write();
                    service
                        .fail(
                            &task,
                            Some(&attempt),
                            LifecycleFailure::processing(
                                job.error
                                    .unwrap_or_else(|| "remote processing failed".to_owned()),
                            ),
                            now,
                        )
                        .await?;
                    report.push(task.id, RecoveryCommandKind::Terminal);
                }
                JobStatus::Cancelled => {
                    let _write = stage.begin_write();
                    service
                        .fail(
                            &task,
                            Some(&attempt),
                            LifecycleFailure::restart_cancelled(
                                "remote job was cancelled during worker restart",
                            ),
                            now,
                        )
                        .await?;
                    report.push(task.id, RecoveryCommandKind::Terminal);
                }
            },
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
            Err(error) => return Err(error.into()),
        }
        Ok(())
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
        && job.workflow_name == task.request.workflow
        && params.get("input") == Some(&Value::String(input.as_str().to_owned()))
        && params.get("output") == Some(&Value::String(output.as_str().to_owned()))
}
