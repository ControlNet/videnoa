use std::collections::{HashMap, HashSet};
use std::time::Duration;

use chrono::Utc;
use tokio::sync::broadcast;
use tokio::task::JoinSet;
use tokio::time::{interval, Instant, MissedTickBehavior};

use crate::domain::{TaskId, TaskStatus};
use crate::lifecycle::JitterSample;
use crate::operations::EventHub;
use crate::persistence::Store;
use crate::recovery::{Reconciler, RecoveryCommandKind, ShutdownCoordinator, StagePermit};
use crate::scheduler::{Scheduler, TransferExecutor};

const SCAN_LIMIT: u16 = u16::MAX;

#[derive(Debug, thiserror::Error)]
pub enum OrchestrationError {
    #[error(transparent)]
    Persistence(#[from] crate::persistence::PersistenceError),
    #[error(transparent)]
    Scheduler(#[from] crate::scheduler::SchedulerError),
    #[error(transparent)]
    Recovery(#[from] crate::recovery::RecoveryError),
    #[error(transparent)]
    Transfer(#[from] crate::scheduler::TransferError),
    #[error("an orchestration stage task terminated unexpectedly")]
    StageJoin(#[from] tokio::task::JoinError),
}

pub struct Orchestrator {
    store: Store,
    scheduler: Scheduler,
    reconciler: Reconciler,
    transfers: TransferExecutor,
    shutdown: ShutdownCoordinator,
    wakeups: broadcast::Receiver<()>,
    poll_interval: Duration,
}

impl Orchestrator {
    #[must_use]
    pub fn new(
        store: Store,
        scheduler: Scheduler,
        reconciler: Reconciler,
        transfers: TransferExecutor,
        shutdown: ShutdownCoordinator,
        events: &EventHub,
    ) -> Self {
        let poll_interval =
            Duration::from_secs(scheduler.runtime_settings().timeout_settings().poll_seconds);
        Self {
            store,
            scheduler,
            reconciler,
            transfers,
            shutdown,
            wakeups: events.subscribe_wakeups(),
            poll_interval,
        }
    }

    /// Advances durable task state until shutdown intake closes.
    ///
    /// # Errors
    /// Returns when durable scheduling cannot be scanned or a stage task panics.
    pub async fn run(mut self) -> Result<(), OrchestrationError> {
        let cancellation = self.shutdown.cancellation_token();
        let mut stages = JoinSet::new();
        let mut active = HashSet::new();
        let mut not_before = HashMap::new();
        self.fill(&mut stages, &mut active, &not_before).await?;
        let mut poll = interval(self.poll_interval);
        poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
        poll.tick().await;

        loop {
            tokio::select! {
                () = cancellation.cancelled() => break,
                received = self.wakeups.recv() => {
                    if matches!(received, Err(broadcast::error::RecvError::Closed)) {
                        break;
                    }
                    self.fill(&mut stages, &mut active, &not_before).await?;
                }
                _ = poll.tick() => {
                    let now = Instant::now();
                    not_before.retain(|_, deadline| *deadline > now);
                    self.fill(&mut stages, &mut active, &not_before).await?;
                }
                joined = stages.join_next(), if !stages.is_empty() => {
                    let Some(joined) = joined else {
                        continue;
                    };
                    let outcome = joined?;
                    active.remove(&outcome.task_id);
                    let defer = match outcome.result {
                        Ok(defer) => defer,
                        Err(error) if error.retryable() => true,
                        Err(StageError::Recovery(error)) => return Err(error.into()),
                        Err(StageError::Transfer(error)) => return Err(error.into()),
                    };
                    if defer {
                        not_before.insert(outcome.task_id, Instant::now() + self.poll_interval);
                    } else {
                        self.fill(&mut stages, &mut active, &not_before).await?;
                    }
                }
            }
        }
        stages.abort_all();
        while let Some(joined) = stages.join_next().await {
            if let Err(error) = joined {
                if !error.is_cancelled() {
                    return Err(error.into());
                }
            }
        }
        Ok(())
    }

    async fn fill(
        &self,
        stages: &mut JoinSet<StageOutcome>,
        active: &mut HashSet<TaskId>,
        not_before: &HashMap<TaskId, Instant>,
    ) -> Result<(), OrchestrationError> {
        while self.scheduler.reserve_next(Utc::now()).await?.is_some() {}
        let now = Utc::now();
        for task in self.store.recovery_tasks(SCAN_LIMIT).await? {
            if task.status == TaskStatus::Queued
                || active.contains(&task.id)
                || not_before.contains_key(&task.id)
                || self.worker_health_deferred(task.worker_id, now).await?
            {
                continue;
            }
            let Some(stage) = self.shutdown.begin_stage() else {
                continue;
            };
            active.insert(task.id);
            let reconciler = self.reconciler.clone();
            let transfers = self.transfers.clone();
            stages.spawn(advance_task(task.id, reconciler, transfers, stage));
        }
        Ok(())
    }

    async fn worker_health_deferred(
        &self,
        worker_id: Option<crate::domain::WorkerId>,
        now: chrono::DateTime<Utc>,
    ) -> Result<bool, crate::persistence::PersistenceError> {
        let Some(worker_id) = worker_id else {
            return Ok(false);
        };
        let Some(worker) = self.store.worker(worker_id).await? else {
            return Ok(false);
        };
        Ok(!worker.online
            && match worker.next_health_check_at {
                Some(deadline) => deadline > now,
                None => worker.health_retry_count > 0,
            })
    }
}

struct StageOutcome {
    task_id: TaskId,
    result: Result<bool, StageError>,
}

#[derive(Debug, thiserror::Error)]
enum StageError {
    #[error(transparent)]
    Recovery(#[from] crate::recovery::RecoveryError),
    #[error(transparent)]
    Transfer(#[from] crate::scheduler::TransferError),
}

impl StageError {
    const fn retryable(&self) -> bool {
        match self {
            Self::Recovery(
                crate::recovery::RecoveryError::Conflict
                | crate::recovery::RecoveryError::LocalCleanup(_),
            )
            | Self::Transfer(
                crate::scheduler::TransferError::Conflict
                | crate::scheduler::TransferError::Busy
                | crate::scheduler::TransferError::RetryNotDue,
            ) => true,
            Self::Recovery(crate::recovery::RecoveryError::Remote(error))
            | Self::Transfer(crate::scheduler::TransferError::Remote(error)) => {
                error.is_transient()
            }
            Self::Recovery(
                crate::recovery::RecoveryError::Persistence(_)
                | crate::recovery::RecoveryError::Lifecycle(_)
                | crate::recovery::RecoveryError::ClientConfig(_)
                | crate::recovery::RecoveryError::Worker(_)
                | crate::recovery::RecoveryError::Scheduler(_)
                | crate::recovery::RecoveryError::MissingAttempt
                | crate::recovery::RecoveryError::MissingWorker
                | crate::recovery::RecoveryError::MissingRemoteEvidence
                | crate::recovery::RecoveryError::HealthDelayRange,
            )
            | Self::Transfer(
                crate::scheduler::TransferError::MissingEvidence
                | crate::scheduler::TransferError::TimeRange
                | crate::scheduler::TransferError::Jitter(_)
                | crate::scheduler::TransferError::Persistence(_)
                | crate::scheduler::TransferError::Lifecycle(_)
                | crate::scheduler::TransferError::Path(_)
                | crate::scheduler::TransferError::ClientConfig(_)
                | crate::scheduler::TransferError::Io(_),
            ) => false,
        }
    }
}

async fn advance_task(
    task_id: TaskId,
    reconciler: Reconciler,
    transfers: TransferExecutor,
    _stage: StagePermit,
) -> StageOutcome {
    StageOutcome {
        task_id,
        result: advance_task_inner(task_id, &reconciler, &transfers).await,
    }
}

async fn advance_task_inner(
    task_id: TaskId,
    reconciler: &Reconciler,
    transfers: &TransferExecutor,
) -> Result<bool, StageError> {
    loop {
        let report = reconciler.reconcile_task_id(task_id, Utc::now()).await?;
        let Some(command) = report.command_kind(task_id) else {
            return Ok(true);
        };
        match command {
            RecoveryCommandKind::Upload
            | RecoveryCommandKind::Download
            | RecoveryCommandKind::Verify
            | RecoveryCommandKind::Publish
            | RecoveryCommandKind::Cleanup => {
                let advanced = transfers
                    .dispatch_recovery(&report, Utc::now(), JitterSample::default())
                    .await?;
                if !advanced.contains(&task_id) {
                    return Ok(true);
                }
            }
            RecoveryCommandKind::Submit | RecoveryCommandKind::Poll => {
                return Ok(true);
            }
            RecoveryCommandKind::AwaitReservation | RecoveryCommandKind::Terminal => {
                return Ok(false);
            }
        }
    }
}
