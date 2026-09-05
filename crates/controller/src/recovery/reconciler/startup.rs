use std::collections::HashSet;

use chrono::{DateTime, Utc};

use crate::recovery::{RecoveryError, RecoveryReport};

use super::Reconciler;

impl Reconciler {
    /// Reconciles every durable nonterminal task independently of future scheduling.
    ///
    /// # Errors
    /// Returns a typed error when durable state cannot be loaded or committed.
    pub async fn reconcile_startup(
        &self,
        now: DateTime<Utc>,
    ) -> Result<RecoveryReport, RecoveryError> {
        let mut report = RecoveryReport::default();
        let Some(mut scan) = self.store.begin_recovery_scan().await? else {
            return Ok(report);
        };
        let mut seen = HashSet::new();
        loop {
            let tasks = self
                .store
                .recovery_tasks(&scan, self.recovery_page_size)
                .await?;
            let Some(last) = tasks.last() else {
                break;
            };
            scan.advance(last);
            for task in tasks {
                if !seen.insert(task.id) {
                    continue;
                }
                let Some(stage) = self.shutdown.begin_stage() else {
                    report.defer(task.id);
                    continue;
                };
                self.reconcile_task(task, now, &stage, &mut report).await?;
            }
        }
        Ok(report)
    }
}
