use chrono::{DateTime, Utc};

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
    pub async fn allows(&self, action: DurableAction) -> Result<bool, SchedulerError> {
        let paused = self.store.settings().await?.scheduler.paused;
        if !paused {
            return Ok(true);
        }
        Ok(match action {
            DurableAction::Upload | DurableAction::Submit => false,
            DurableAction::None
            | DurableAction::Poll
            | DurableAction::Download
            | DurableAction::Verify
            | DurableAction::Publish
            | DurableAction::Cleanup
            | DurableAction::Cancel(_) => true,
        })
    }

    /// Updates persisted scheduler settings and live transfer bounds.
    ///
    /// # Errors
    /// Returns a typed conflict when the settings snapshot is stale.
    pub async fn update_settings(&self, update: SettingsUpdate) -> Result<(), SchedulerError> {
        RuntimeSettings::new(&update.timeouts, &update.retry)?;
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
