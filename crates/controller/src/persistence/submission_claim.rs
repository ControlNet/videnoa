use chrono::{DateTime, Utc};

use super::codec::{sqlite_u64, timestamp};
use super::{
    AttemptRecord, DurableChange, PersistenceError, Store, SubmissionClaim, SubmissionClaimOutcome,
    SubmissionOwner,
};

impl Store {
    pub(crate) async fn claim_submission(
        &self,
        claim: SubmissionClaim,
    ) -> Result<SubmissionClaimOutcome, PersistenceError> {
        let owner = claim.owner.to_string();
        let result = sqlx::query(
            "UPDATE task_attempts SET submission_owner = ?, version = version + 1,
                updated_at_ms = ?, next_retry_at_ms = NULL
             WHERE id = ? AND status = 'submitting' AND version = ?
               AND (submission_owner IS NULL OR submission_owner != ?)
               AND (next_retry_at_ms IS NULL OR next_retry_at_ms <= ?)",
        )
        .bind(&owner)
        .bind(timestamp(claim.claimed_at))
        .bind(claim.attempt_id.to_string())
        .bind(sqlite_u64("expected_version", claim.expected_version)?)
        .bind(&owner)
        .bind(timestamp(claim.claimed_at))
        .execute(self.database.pool())
        .await?;
        if result.rows_affected() == 1 {
            return Ok(SubmissionClaimOutcome::Claimed {
                new_version: claim.expected_version + 1,
            });
        }
        let current: Option<Option<String>> = sqlx::query_scalar(
            "SELECT submission_owner FROM task_attempts
             WHERE id = ? AND status = 'submitting' AND version = ?",
        )
        .bind(claim.attempt_id.to_string())
        .bind(sqlite_u64("expected_version", claim.expected_version)?)
        .fetch_optional(self.database.pool())
        .await?;
        match current {
            Some(Some(current)) if current == owner => Ok(SubmissionClaimOutcome::Owned),
            Some(None | Some(_)) | None => Ok(SubmissionClaimOutcome::Conflict),
        }
    }

    /// Releases only the finished request's claim and persists its retry deadline.
    pub(crate) async fn schedule_submission_retry(
        &self,
        attempt: &AttemptRecord,
        owner: SubmissionOwner,
        retry_at: DateTime<Utc>,
        occurred_at: DateTime<Utc>,
    ) -> Result<bool, PersistenceError> {
        let retry_count = attempt.attempt.retry.retry_count.saturating_add(1);
        let mut transaction = self.database.pool().begin().await?;
        let result = sqlx::query(
            "UPDATE task_attempts SET submission_owner = NULL, retry_count = ?,
                next_retry_at_ms = ?, version = version + 1, updated_at_ms = ?
             WHERE id = ? AND status = 'submitting' AND version = ?
               AND submission_owner = ? AND remote_job_id IS NULL",
        )
        .bind(i64::from(retry_count))
        .bind(timestamp(retry_at))
        .bind(timestamp(occurred_at))
        .bind(attempt.attempt.id.to_string())
        .bind(sqlite_u64("attempt_version", attempt.version)?)
        .bind(owner.to_string())
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(false);
        }
        // Cancellation can change the task version while a submission is in flight.
        // The claim CAS above and current attempt number fence this paired update.
        let result = sqlx::query(
            "UPDATE tasks SET retry_count = ?, next_retry_at_ms = ?,
                version = version + 1, updated_at_ms = ?
             WHERE id = ? AND status = 'submitting' AND attempt_count = ?",
        )
        .bind(i64::from(retry_count))
        .bind(timestamp(retry_at))
        .bind(timestamp(occurred_at))
        .bind(attempt.attempt.task_id.to_string())
        .bind(i64::from(attempt.attempt.attempt_number))
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(false);
        }
        transaction.commit().await?;
        self.notify_change(DurableChange::Task(attempt.attempt.task_id));
        Ok(true)
    }
}
