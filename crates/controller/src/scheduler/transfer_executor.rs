use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::domain::{RetryMetadata, TaskId};
use crate::lifecycle::{
    AutomaticRetry, DownstreamFailure, JitterSample, LifecycleFailure, LifecycleService,
    RetryDecision, RetryPolicy, UploadEvidence,
};
use crate::paths::PathCapabilities;
use crate::persistence::{Sha256Digest, Store};
use crate::remote::{PayloadLimits, RemoteTimeouts};

use super::{TransferCoordinator, TransferError};

#[derive(Clone)]
pub struct TransferResources {
    pub store: Store,
    pub paths: PathCapabilities,
    pub coordinator: TransferCoordinator,
}

#[derive(Clone, Debug)]
pub struct TransferConfig {
    pub temp_root: PathBuf,
    pub remote_timeouts: RemoteTimeouts,
    pub payload_limits: PayloadLimits,
    pub retry_policy: RetryPolicy,
}

#[derive(Clone)]
pub struct TransferExecutor {
    pub(super) resources: TransferResources,
    pub(super) config: TransferConfig,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedArtifact {
    pub path: PathBuf,
    pub size: u64,
    pub sha256: Sha256Digest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DownloadOutcome {
    Verified(VerifiedArtifact),
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
    pub const fn new(resources: TransferResources, config: TransferConfig) -> Self {
        Self { resources, config }
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
        match self
            .config
            .retry_policy
            .decide(operation, task.retry.retry_count, jitter)
        {
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
