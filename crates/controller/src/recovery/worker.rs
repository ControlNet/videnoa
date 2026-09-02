use chrono::{DateTime, Utc};

use crate::domain::TaskId;
use crate::persistence::{WorkerHealthUpdate, WorkerRecord};
use crate::workers::WorkerRegistry;

use super::{Reconciler, RecoveryError, RecoveryReport, StagePermit};

impl Reconciler {
    pub(super) async fn defer_worker(
        &self,
        worker: &WorkerRecord,
        task_id: TaskId,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        if worker.health_retry_count >= self.config.health_max_attempts {
            report.defer(task_id);
            return Ok(());
        }
        let retry_count = worker.health_retry_count + 1;
        let exhausted = retry_count >= self.config.health_max_attempts;
        let exponent = worker.health_retry_count;
        let multiplier = 1_u32.checked_shl(exponent).unwrap_or(u32::MAX);
        let delay = self
            .config
            .health_initial
            .saturating_mul(multiplier)
            .min(self.config.health_maximum);
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
        let _write = stage.begin_write();
        WorkerRegistry::new(self.store.clone())
            .refresh_health(WorkerHealthUpdate {
                id: worker.id,
                expected_version: worker.version,
                online: false,
                capabilities: worker.capabilities.clone(),
                last_seen_at: worker.last_seen_at,
                health_retry_count: retry_count,
                next_health_check_at,
                last_error: Some(last_error.to_owned()),
                updated_at: now,
            })
            .await?;
        report.defer(task_id);
        Ok(())
    }
}
