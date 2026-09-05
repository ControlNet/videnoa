use chrono::{DateTime, Utc};

use crate::lifecycle::{DownstreamFailure, LifecycleFailure, LifecycleService};
use crate::paths::PathError;
use crate::persistence::{AttemptRecord, Sha256Digest, TaskRecord};

use super::{TransferError, TransferExecutor};

#[derive(Clone, Copy)]
pub(super) struct ExpectedPublication {
    pub size: u64,
    pub sha256: Sha256Digest,
}

impl TransferExecutor {
    pub(super) async fn fail_verification(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
    ) -> Result<bool, TransferError> {
        LifecycleService::new(self.resources.store.clone())
            .fail(
                task,
                Some(attempt),
                LifecycleFailure::downstream(
                    DownstreamFailure::Verification,
                    "verified artifact does not match durable evidence",
                ),
                now,
            )
            .await?;
        Ok(false)
    }

    pub(super) async fn fail_publication_path(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        _error: PathError,
        now: DateTime<Utc>,
    ) -> Result<bool, TransferError> {
        if task.status == crate::domain::TaskStatus::Verifying {
            LifecycleService::new(self.resources.store.clone())
                .fail(
                    task,
                    Some(attempt),
                    LifecycleFailure::publication_admission(
                        "publication destination capability could not be opened",
                    ),
                    now,
                )
                .await?;
            return Ok(false);
        }
        self.fail_publication(task, attempt, now).await
    }

    pub(super) async fn fail_publication(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
    ) -> Result<bool, TransferError> {
        LifecycleService::new(self.resources.store.clone())
            .fail(
                task,
                Some(attempt),
                LifecycleFailure::downstream(
                    DownstreamFailure::Publication,
                    "publication filesystem operation failed",
                ),
                now,
            )
            .await?;
        Ok(false)
    }

    pub(super) async fn fail_ambiguous(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
    ) -> Result<bool, TransferError> {
        LifecycleService::new(self.resources.store.clone())
            .fail(
                task,
                Some(attempt),
                LifecycleFailure::publication_ambiguous(
                    "durable publication evidence does not identify owned output bytes",
                ),
                now,
            )
            .await?;
        Ok(false)
    }
}

pub(super) fn publication_evidence(task: &TaskRecord) -> Option<ExpectedPublication> {
    Some(ExpectedPublication {
        size: task.publication.expected_output_size?,
        sha256: task.publication.expected_output_sha256?,
    })
}
