use chrono::{DateTime, Utc};

use crate::lifecycle::{DownstreamFailure, LifecycleFailure, LifecycleService};
use crate::persistence::{AttemptRecord, Sha256Digest, TaskRecord};

use super::diagnostics::OperationError;
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
        diagnostic: OperationError,
    ) -> Result<bool, TransferError> {
        diagnostic.log_failure(task.id, attempt.attempt.id, task.status);
        LifecycleService::new(self.resources.store.clone())
            .fail(
                task,
                Some(attempt),
                LifecycleFailure::downstream(DownstreamFailure::Verification, diagnostic.summary()),
                now,
            )
            .await?;
        Ok(false)
    }

    pub(super) async fn fail_publication_path(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
        diagnostic: OperationError,
    ) -> Result<bool, TransferError> {
        if task.status == crate::domain::TaskStatus::Verifying {
            diagnostic.log_failure(task.id, attempt.attempt.id, task.status);
            LifecycleService::new(self.resources.store.clone())
                .fail(
                    task,
                    Some(attempt),
                    LifecycleFailure::publication_admission(diagnostic.summary()),
                    now,
                )
                .await?;
            return Ok(false);
        }
        self.fail_publication(task, attempt, now, diagnostic).await
    }

    pub(super) async fn fail_publication(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
        diagnostic: OperationError,
    ) -> Result<bool, TransferError> {
        diagnostic.log_failure(task.id, attempt.attempt.id, task.status);
        LifecycleService::new(self.resources.store.clone())
            .fail(
                task,
                Some(attempt),
                LifecycleFailure::downstream(DownstreamFailure::Publication, diagnostic.summary()),
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
        diagnostic: OperationError,
    ) -> Result<bool, TransferError> {
        diagnostic.log_failure(task.id, attempt.attempt.id, task.status);
        LifecycleService::new(self.resources.store.clone())
            .fail(
                task,
                Some(attempt),
                LifecycleFailure::publication_ambiguous(diagnostic.summary()),
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
