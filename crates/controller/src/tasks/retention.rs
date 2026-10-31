//! Keeps the Controller's history bounded: batch `Idempotency-Key` receipts
//! always expire, and finished tasks are purged only when
//! `history.retention_days` is configured.

use std::num::NonZeroU32;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use tokio::time::{interval, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use crate::config::HistoryConfig;
use crate::domain::TaskId;
use crate::paths::PathCapabilities;
use crate::persistence::{DurableChange, PersistenceError, Store};
use crate::scheduler::{local_files_unneeded, remove_task_workspace};

/// How often history is swept.
pub const HISTORY_SWEEP_INTERVAL: Duration = Duration::from_hours(1);
/// Days a batch `Idempotency-Key` replays its stored response.
pub const BATCH_RECEIPT_DAYS: i64 = 7;
const PURGE_PAGE: u32 = 200;

/// What one sweep removed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetentionSweep {
    pub expired_batch_receipts: u64,
    pub purged_tasks: u64,
}

#[derive(Clone)]
pub struct HistoryRetention {
    store: Store,
    paths: PathCapabilities,
    retention_days: Option<NonZeroU32>,
}

impl HistoryRetention {
    #[must_use]
    pub const fn new(store: Store, paths: PathCapabilities, config: HistoryConfig) -> Self {
        Self {
            store,
            paths,
            retention_days: config.retention_days,
        }
    }

    /// Sweeps history every `cadence` until `shutdown`.
    pub async fn run(self, cadence: Duration, shutdown: CancellationToken) {
        let mut ticks = interval(cadence);
        ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return,
                _ = ticks.tick() => {}
            }
            match self.sweep(Utc::now()).await {
                Ok(sweep) => tracing::debug!(
                    expired_batch_receipts = sweep.expired_batch_receipts,
                    purged_tasks = sweep.purged_tasks,
                    "Swept task history"
                ),
                Err(error) => tracing::warn!(%error, "Task history sweep failed"),
            }
        }
    }

    /// Expires old batch receipts and, when retention is configured, purges
    /// finished tasks that nothing can use any more.
    ///
    /// # Errors
    /// Returns [`PersistenceError`] when history cannot be read or deleted.
    pub async fn sweep(&self, now: DateTime<Utc>) -> Result<RetentionSweep, PersistenceError> {
        let expired_batch_receipts = self
            .store
            .expire_batch_receipts(now - TimeDelta::days(BATCH_RECEIPT_DAYS))
            .await?;
        let purged_tasks = match self.retention_days {
            Some(days) => {
                self.purge_finished(now - TimeDelta::days(i64::from(days.get())))
                    .await?
            }
            None => 0,
        };
        Ok(RetentionSweep {
            expired_batch_receipts,
            purged_tasks,
        })
    }

    async fn purge_finished(&self, cutoff: DateTime<Utc>) -> Result<u64, PersistenceError> {
        let mut purged = 0;
        let mut after: Option<TaskId> = None;
        let mut last_purged = None;
        loop {
            let page = self
                .store
                .finished_tasks_updated_before(cutoff, after, PURGE_PAGE)
                .await?;
            let Some(tail) = page.last() else {
                break;
            };
            after = Some(tail.id);
            for task in page {
                // A failure Retry can still resume keeps its record and files.
                if !local_files_unneeded(&task) {
                    continue;
                }
                // Files go first: once the row is gone, nothing releases them.
                if let Err(error) = remove_task_workspace(&self.paths, task.id).await {
                    tracing::warn!(task_id = %task.id, %error, "Expired task kept its temporary files");
                    continue;
                }
                if self
                    .store
                    .purge_finished_task(task.id, task.version)
                    .await?
                {
                    purged += 1;
                    last_purged = Some(task.id);
                }
            }
        }
        if let Some(task_id) = last_purged {
            // A missing task makes clients refetch their lists.
            self.store.notify_change(DurableChange::Task(task_id));
        }
        Ok(purged)
    }
}
