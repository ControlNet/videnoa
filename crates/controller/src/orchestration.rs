use std::num::NonZeroU16;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::broadcast;

use crate::domain::TaskId;
use crate::lifecycle::JitterSample;
use crate::operations::EventHub;
use crate::persistence::Store;
use crate::recovery::{Reconciler, RecoveryCommandKind, ShutdownCoordinator, StagePermit};
use crate::scheduler::{Scheduler, TransferExecutor};

mod recovery_scan;

const RECOVERY_PAGE_SIZE: NonZeroU16 = match NonZeroU16::new(256) {
    Some(value) => value,
    None => NonZeroU16::MIN,
};

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
    recovery_page_size: NonZeroU16,
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
            recovery_page_size: RECOVERY_PAGE_SIZE,
        }
    }

    #[must_use]
    pub fn with_recovery_page_size(mut self, page_size: NonZeroU16) -> Self {
        self.recovery_page_size = page_size;
        self
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
