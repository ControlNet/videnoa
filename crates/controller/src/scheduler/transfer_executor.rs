use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::time::Instant;

use crate::domain::{RetryMetadata, TaskId};
use crate::lifecycle::{
    AutomaticRetry, DownstreamFailure, JitterSample, LifecycleFailure, LifecycleService,
    RetryDecision, UploadEvidence,
};
use crate::paths::PathCapabilities;
use crate::persistence::{Sha256Digest, Store};
use crate::remote::PayloadLimits;

use super::checkpoints::noop_observer;
use super::{
    TransferCheckpointObserver, TransferCheckpointPoint, TransferCoordinator, TransferError,
};

pub type TransferClock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;
pub type JitterSource = Arc<dyn Fn() -> JitterSample + Send + Sync>;

#[derive(Clone)]
pub struct TransferResources {
    pub store: Store,
    pub paths: PathCapabilities,
    pub coordinator: TransferCoordinator,
}

#[derive(Clone, Debug)]
pub struct TransferConfig {
    pub payload_limits: PayloadLimits,
    pub runtime_settings: super::RuntimeSettings,
}

#[derive(Clone)]
pub struct TransferExecutor {
    pub(super) resources: TransferResources,
    pub(super) config: TransferConfig,
    checkpoint_observer: Arc<dyn TransferCheckpointObserver>,
    clock: Option<TransferClock>,
    jitter_source: JitterSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UploadOutcome {
    Staged(UploadEvidence),
    RetryScheduled {
        retry_count: u32,
        next_retry_at: DateTime<Utc>,
    },
    Failed,
}

#[derive(Clone)]
pub struct VerifiedArtifact {
    pub path: PathBuf,
    pub size: u64,
    pub sha256: Sha256Digest,
    pub(crate) source: crate::paths::TempArtifact,
}

#[derive(Clone)]
pub enum DownloadOutcome {
    Verified(Box<VerifiedArtifact>),
    RetryScheduled {
        retry_count: u32,
        next_retry_at: DateTime<Utc>,
    },
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PublicationOutcome {
    Completed,
    RetryScheduled {
        retry_count: u32,
        next_retry_at: DateTime<Utc>,
    },
    Failed,
}

impl TransferExecutor {
    #[must_use]
    pub fn new(resources: TransferResources, config: TransferConfig) -> Self {
        Self {
            resources,
            config,
            checkpoint_observer: noop_observer(),
            clock: None,
            jitter_source: Arc::new(JitterSample::random),
        }
    }

    #[must_use]
    pub fn with_checkpoint_observer(
        mut self,
        observer: Arc<dyn TransferCheckpointObserver>,
    ) -> Self {
        self.checkpoint_observer = observer;
        self
    }

    #[must_use]
    pub fn with_clock(mut self, clock: TransferClock) -> Self {
        self.clock = Some(clock);
        self
    }

    #[must_use]
    pub fn with_jitter_source(mut self, jitter_source: JitterSource) -> Self {
        self.jitter_source = jitter_source;
        self
    }

    /// Reads the injected clock when configured, or advances `started_at` by
    /// the elapsed duration of the transfer stage so post-I/O failure/completion
    /// timestamps reflect actual transfer time while keeping deterministic test
    /// timestamps anchored.
    pub(super) fn observed_now(
        &self,
        started_at: DateTime<Utc>,
        started: Instant,
    ) -> DateTime<Utc> {
        if let Some(clock) = &self.clock {
            return clock();
        }
        chrono::Duration::from_std(started.elapsed())
            .ok()
            .and_then(|elapsed| started_at.checked_add_signed(elapsed))
            .unwrap_or(started_at)
    }

    pub(super) fn live_now(&self) -> DateTime<Utc> {
        self.clock.as_ref().map_or_else(Utc::now, |clock| clock())
    }

    pub(super) fn sample_jitter(&self) -> JitterSample {
        (self.jitter_source)()
    }

    pub(super) async fn checkpoint(&self, point: TransferCheckpointPoint) {
        self.checkpoint_observer.checkpoint(point).await;
    }

    pub(super) async fn snapshots(
        &self,
        task_id: TaskId,
    ) -> Result<
        (
            crate::persistence::TaskRecord,
            crate::persistence::AttemptRecord,
        ),
        TransferError,
    > {
        let task = self
            .resources
            .store
            .task(task_id)
            .await?
            .ok_or(TransferError::MissingEvidence)?;
        let attempt = self
            .resources
            .store
            .current_attempt(task_id)
            .await?
            .ok_or(TransferError::MissingEvidence)?;
        Ok((task, attempt))
    }

    pub(super) fn require_retry_due(
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        now: DateTime<Utc>,
    ) -> Result<(), TransferError> {
        if task.retry != attempt.attempt.retry {
            return Err(TransferError::MissingEvidence);
        }
        if task
            .retry
            .next_retry_at
            .is_some_and(|deadline| deadline > now)
        {
            return Err(TransferError::RetryNotDue);
        }
        Ok(())
    }

    pub(super) async fn retry(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        operation: AutomaticRetry,
        failure: DownstreamFailure,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<RetryResult, TransferError> {
        match self.config.runtime_settings.retry_policy().decide(
            operation,
            task.retry.retry_count,
            jitter,
        ) {
            RetryDecision::Schedule {
                retry_count, delay, ..
            } => {
                let next_retry_at = now
                    .checked_add_signed(
                        chrono::Duration::from_std(delay).map_err(|_| TransferError::TimeRange)?,
                    )
                    .ok_or(TransferError::TimeRange)?;
                LifecycleService::new(self.resources.store.clone())
                    .schedule_transfer_retry(
                        task,
                        attempt,
                        retry_metadata(retry_count, next_retry_at),
                        now,
                    )
                    .await?;
                Ok(RetryResult::Scheduled {
                    retry_count,
                    next_retry_at,
                })
            }
            RetryDecision::Exhausted => {
                LifecycleService::new(self.resources.store.clone())
                    .fail(
                        task,
                        Some(attempt),
                        LifecycleFailure::downstream(failure, "transfer retry limit exhausted"),
                        now,
                    )
                    .await?;
                Ok(RetryResult::Failed)
            }
        }
    }
}

pub(super) enum RetryResult {
    Scheduled {
        retry_count: u32,
        next_retry_at: DateTime<Utc>,
    },
    Failed,
}

pub(super) fn retry_metadata(retry_count: u32, next_retry_at: DateTime<Utc>) -> RetryMetadata {
    RetryMetadata {
        retry_count,
        next_retry_at: Some(next_retry_at),
    }
}
