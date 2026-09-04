use super::codec::{sqlite_u64, timestamp};
use super::{PersistenceError, Store, SubmissionClaim, SubmissionClaimOutcome};

impl Store {
    pub(crate) async fn claim_submission(
        &self,
        claim: SubmissionClaim,
    ) -> Result<SubmissionClaimOutcome, PersistenceError> {
        let owner = claim.owner.to_string();
        let result = sqlx::query(
            "UPDATE task_attempts SET submission_owner = ?, version = version + 1,
                updated_at_ms = ?
             WHERE id = ? AND status = 'submitting' AND version = ?
               AND (submission_owner IS NULL OR submission_owner != ?)",
        )
        .bind(&owner)
        .bind(timestamp(claim.claimed_at))
        .bind(claim.attempt_id.to_string())
        .bind(sqlite_u64("expected_version", claim.expected_version)?)
        .bind(&owner)
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
}
