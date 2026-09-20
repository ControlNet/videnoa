use chrono::{DateTime, Utc};

use crate::persistence::{AttemptRecord, SubmissionClaim, SubmissionClaimOutcome};

use super::{Reconciler, RecoveryError, StagePermit};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SubmissionOwnership {
    Claimed,
    Owned,
}

impl Reconciler {
    pub(super) async fn claim_submission(
        &self,
        attempt: &mut AttemptRecord,
        claimed_at: DateTime<Utc>,
    ) -> Result<SubmissionOwnership, RecoveryError> {
        if attempt
            .attempt
            .retry
            .next_retry_at
            .is_some_and(|deadline| deadline > claimed_at)
        {
            return Ok(SubmissionOwnership::Owned);
        }
        match self
            .store
            .claim_submission(SubmissionClaim {
                attempt_id: attempt.attempt.id,
                expected_version: attempt.version,
                owner: self.submission_owner,
                claimed_at,
            })
            .await?
        {
            SubmissionClaimOutcome::Claimed { new_version } => {
                attempt.version = new_version;
                attempt.attempt.retry.next_retry_at = None;
                Ok(SubmissionOwnership::Claimed)
            }
            SubmissionClaimOutcome::Owned => Ok(SubmissionOwnership::Owned),
            SubmissionClaimOutcome::Conflict => Err(RecoveryError::Conflict),
        }
    }

    pub(super) async fn retry_submission(
        &self,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
        stage: &StagePermit,
        error: &(dyn std::fmt::Display + Sync),
    ) -> Result<(), RecoveryError> {
        // Acceptance is uncertain: keep confirming beyond the transfer retry limit.
        let retry = self.store.config_manager().config().retry;
        let multiplier = 1_u32
            .checked_shl(attempt.attempt.retry.retry_count)
            .unwrap_or(u32::MAX);
        let delay = retry.initial.saturating_mul(multiplier).min(retry.maximum);
        // Start backoff after the request ends, not at its potentially old start time.
        let failed_at = Utc::now().max(now);
        let retry_at = failed_at
            .checked_add_signed(
                chrono::Duration::from_std(delay)
                    .map_err(|_| RecoveryError::SubmissionDelayRange)?,
            )
            .ok_or(RecoveryError::SubmissionDelayRange)?;
        let _write = stage.begin_write();
        if self
            .store
            .schedule_submission_retry(attempt, self.submission_owner, retry_at, failed_at)
            .await?
        {
            tracing::warn!(
                task_id = %attempt.attempt.task_id,
                attempt_id = %attempt.attempt.id,
                retry_count = attempt.attempt.retry.retry_count.saturating_add(1),
                next_retry_at = %retry_at,
                error = %error,
                "Submission confirmation retry scheduled"
            );
        }
        Ok(())
    }
}
