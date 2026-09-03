use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::{OwnedRwLockReadGuard, RwLock};

use crate::domain::{AttemptId, SubmissionKey};
use crate::lifecycle::{DurableAction, LifecycleErrorCode, LifecycleService, ReserveCommand};
use crate::persistence::{CasOutcome, DurableChange, SettingsRecord, SettingsUpdate, Store};

use super::{
    AssignmentClass, RuntimeSettings, ScheduledAssignment, SchedulerError, TransferCoordinator,
    UploadCandidate, UploadPriority,
};

#[derive(Clone, Debug)]
pub struct Scheduler {
    store: Store,
    lifecycle: LifecycleService,
    transfers: TransferCoordinator,
    runtime_settings: RuntimeSettings,
    submission_admission: Arc<RwLock<()>>,
}

impl Scheduler {
    /// Loads persisted scheduler status and initializes ephemeral transfer guards.
    ///
    /// # Errors
    /// Returns an error when persisted settings cannot be loaded.
    pub async fn load(store: Store) -> Result<Self, SchedulerError> {
        let settings = store.settings().await?;
        Ok(Self {
            lifecycle: LifecycleService::new(store.clone()),
            transfers: TransferCoordinator::from_status(&settings.scheduler),
            runtime_settings: RuntimeSettings::new(&settings.timeouts, &settings.retry)?,
            submission_admission: store.submission_admission(),
            store,
        })
    }

    /// Selects and atomically reserves the next eligible task/worker pair.
    ///
    /// # Errors
    /// Returns an error when selection or durable reservation fails.
    pub async fn reserve_next(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Option<ScheduledAssignment>, SchedulerError> {
        loop {
            let Some(candidate) = self.store.scheduler_candidate().await? else {
                return Ok(None);
            };
            let attempt_id = AttemptId::random();
            let submission_key = SubmissionKey::random();
            let result = self
                .lifecycle
                .reserve(&ReserveCommand {
                    task_id: candidate.task_id,
                    expected_task_version: candidate.task_version,
                    worker_id: candidate.worker_id,
                    attempt_id,
                    submission_key,
                    reserved_at: now,
                })
                .await;
            match result {
                Ok(_) => {
                    let class = if candidate.idle_feed {
                        AssignmentClass::IdleFeed
                    } else {
                        AssignmentClass::Prefetch
                    };
                    return Ok(Some(ScheduledAssignment::new(
                        candidate.task_id,
                        candidate.worker_id,
                        attempt_id,
                        submission_key,
                        class,
                    )));
                }
                Err(error) if error.code() == LifecycleErrorCode::Conflict => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// Returns pause-aware reserved upload work in deterministic priority order.
    ///
    /// # Errors
    /// Returns an error when the durable upload queue cannot be read.
    pub async fn upload_candidates(
        &self,
        limit: u16,
    ) -> Result<Vec<UploadCandidate>, SchedulerError> {
        let candidates = self
            .store
            .upload_candidates(limit)
            .await?
            .into_iter()
            .map(|candidate| {
                let priority = if candidate.idle_feed {
                    UploadPriority::IdleFeed
                } else {
                    UploadPriority::Prefetch
                };
                UploadCandidate::new(candidate.task_id, candidate.worker_id, priority)
            })
            .collect();
        Ok(candidates)
    }

    /// Evaluates whether persisted pause permits a durable side effect.
    ///
    /// # Errors
    /// Returns an error when persisted settings cannot be read.
    pub(crate) async fn admit(
        &self,
        action: DurableAction,
    ) -> Result<Option<OwnedRwLockReadGuard<()>>, SchedulerError> {
        let admission = Arc::clone(&self.submission_admission).read_owned().await;
        let paused = self.store.settings().await?.scheduler.paused;
        if !paused {
            return Ok(Some(admission));
        }
        Ok(match action {
            DurableAction::Upload | DurableAction::Submit => None,
            DurableAction::None
            | DurableAction::Poll
            | DurableAction::Download
            | DurableAction::Verify
            | DurableAction::Publish
            | DurableAction::Cleanup
            | DurableAction::Cancel(_) => Some(admission),
        })
    }

    /// Returns a point-in-time pause decision without retaining admission.
    ///
    /// # Errors
    /// Returns an error when persisted settings cannot be read.
    pub async fn allows(&self, action: DurableAction) -> Result<bool, SchedulerError> {
        Ok(self.admit(action).await?.is_some())
    }

    /// Updates persisted scheduler settings and live transfer bounds.
    ///
    /// # Errors
    /// Returns a typed conflict when the settings snapshot is stale.
    pub async fn update_settings(&self, update: SettingsUpdate) -> Result<(), SchedulerError> {
        RuntimeSettings::new(&update.timeouts, &update.retry)?;
        let _admission = Arc::clone(&self.submission_admission).write_owned().await;
        match self.store.update_settings(&update).await? {
            CasOutcome::Applied { .. } => {
                self.transfers.reconfigure(&update.scheduler);
                self.runtime_settings
                    .reconfigure(&update.timeouts, &update.retry)?;
                self.store.notify_change(DurableChange::Settings);
                Ok(())
            }
            CasOutcome::Conflict => Err(SchedulerError::Conflict),
        }
    }

    pub(crate) async fn settings(&self) -> Result<SettingsRecord, SchedulerError> {
        self.store.settings().await.map_err(Into::into)
    }

    #[must_use]
    pub const fn transfers(&self) -> &TransferCoordinator {
        &self.transfers
    }

    #[must_use]
    pub const fn runtime_settings(&self) -> &RuntimeSettings {
        &self.runtime_settings
    }
}

#[cfg(test)]
mod tests {
    use std::future::{poll_fn, Future};
    use std::task::Poll;

    use chrono::Utc;
    use tempfile::TempDir;

    use super::Scheduler;
    use crate::lifecycle::DurableAction;
    use crate::persistence::{Database, DatabaseOptions, SettingsUpdate, Store};

    #[tokio::test]
    async fn settings_update_waits_for_admitted_submit() -> Result<(), Box<dyn std::error::Error>> {
        // Given: one remote submission admitted while the scheduler is unpaused.
        let directory = TempDir::new()?;
        let database = Database::open(DatabaseOptions::new(
            directory.path().join("controller.sqlite3"),
        ))
        .await?;
        let store = Store::new(database);
        let scheduler = Scheduler::load(store.clone()).await?;
        let admission = scheduler
            .admit(DurableAction::Submit)
            .await?
            .ok_or_else(|| std::io::Error::other("submit was not admitted"))?;
        let settings = store.settings().await?;
        let mut status = settings.scheduler;
        status.paused = true;
        let mut update = Box::pin(scheduler.update_settings(SettingsUpdate {
            expected_version: settings.version,
            scheduler: status,
            timeouts: settings.timeouts,
            retry: settings.retry,
            updated_at: Utc::now(),
        }));

        // When: pause persistence is polled while submission owns admission.
        let pending =
            poll_fn(|context| Poll::Ready(matches!(update.as_mut().poll(context), Poll::Pending)))
                .await;
        assert!(
            pending,
            "pause committed before admitted submission completed"
        );
        drop(admission);
        update.await?;

        // Then: pause commits only after the admitted submission boundary closes.
        assert!(store.settings().await?.scheduler.paused);
        Ok(())
    }
}
