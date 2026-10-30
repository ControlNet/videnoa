use chrono::{DateTime, Utc};

use crate::domain::TaskId;
use crate::persistence::{WorkerHealthUpdate, WorkerRecord};
use crate::remote::VidenoaClientError;
use crate::workers::{WorkerRegistry, WorkerRegistryError, AUTHENTICATION_FAILED};

use super::{Reconciler, RecoveryError, RecoveryReport, StagePermit};

impl Reconciler {
    /// Defers a task after its worker failed a request with a reply that is
    /// no evidence about the task's job. Transient faults keep the per-task
    /// cooldown, and Controller-local faults stay errors.
    pub(super) async fn defer_remote(
        &self,
        worker: &WorkerRecord,
        task_id: TaskId,
        error: VidenoaClientError,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        match error {
            VidenoaClientError::LocalIo
            | VidenoaClientError::InvalidFilePath
            | VidenoaClientError::EndpointUrl => Err(error.into()),
            error if error.is_transient() => Err(error.into()),
            error => {
                tracing::warn!(worker_id = %worker.id, %task_id, %error, "Worker rejected a request; task deferred");
                self.defer_for_worker(worker, task_id, &error, now, stage, report)
                    .await
            }
        }
    }

    /// Defers a task because its worker, not the task, is failing: rejected
    /// credentials block the worker, anything else backs it off.
    pub(super) async fn defer_for_worker(
        &self,
        worker: &WorkerRecord,
        task_id: TaskId,
        error: &VidenoaClientError,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        if error.is_authentication() {
            // Like the health service: offline with no next check, so the
            // worker's tasks wait until a probe sees the credentials work.
            let update = offline(
                worker,
                worker.health_retry_count.max(1),
                None,
                AUTHENTICATION_FAILED,
                now,
            );
            self.refresh_worker(update, task_id, stage, report).await
        } else {
            self.defer_worker(worker, task_id, now, stage, report)
                .await
        }
    }

    pub(super) async fn defer_worker(
        &self,
        worker: &WorkerRecord,
        task_id: TaskId,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        let (health_initial, health_maximum, health_max_attempts) = self.config.health_retry();
        if worker.health_retry_count >= health_max_attempts {
            report.defer(task_id);
            return Ok(());
        }
        let retry_count = worker.health_retry_count + 1;
        let exhausted = retry_count >= health_max_attempts;
        let exponent = worker.health_retry_count;
        let multiplier = 1_u32.checked_shl(exponent).unwrap_or(u32::MAX);
        let delay = health_initial
            .saturating_mul(multiplier)
            .min(health_maximum);
        let next_health_check_at = if exhausted {
            None
        } else {
            Some(
                now.checked_add_signed(
                    chrono::Duration::from_std(delay)
                        .map_err(|_| RecoveryError::HealthDelayRange)?,
                )
                .ok_or(RecoveryError::HealthDelayRange)?,
            )
        };
        let last_error = if exhausted {
            "worker health recovery retry bound exhausted"
        } else {
            "worker health check failed during recovery"
        };
        let update = offline(worker, retry_count, next_health_check_at, last_error, now);
        self.refresh_worker(update, task_id, stage, report).await
    }

    async fn refresh_worker(
        &self,
        update: WorkerHealthUpdate,
        task_id: TaskId,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        let worker_id = update.id;
        let _write = stage.begin_write();
        let result = WorkerRegistry::new(self.store.clone())
            .refresh_health(update)
            .await;
        match result {
            Ok(_) => {}
            Err(WorkerRegistryError::Conflict) => {
                // A concurrent edit or health probe superseded this worker snapshot.
                // Preserve the newer record and retry the task with fresh state.
                tracing::debug!(%worker_id, %task_id, "Worker changed during recovery health update; task deferred");
            }
            Err(error) => return Err(error.into()),
        }
        report.defer(task_id);
        Ok(())
    }
}

fn offline(
    worker: &WorkerRecord,
    health_retry_count: u32,
    next_health_check_at: Option<DateTime<Utc>>,
    last_error: &str,
    now: DateTime<Utc>,
) -> WorkerHealthUpdate {
    WorkerHealthUpdate {
        id: worker.id,
        expected_version: worker.version,
        online: false,
        capabilities: worker.capabilities.clone(),
        last_seen_at: worker.last_seen_at,
        health_retry_count,
        next_health_check_at,
        last_error: Some(last_error.to_owned()),
        updated_at: now,
    }
}
