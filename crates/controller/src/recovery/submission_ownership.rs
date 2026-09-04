use chrono::{DateTime, Utc};

use crate::persistence::{AttemptRecord, SubmissionClaim, SubmissionClaimOutcome};

use super::{Reconciler, RecoveryError};

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
                Ok(SubmissionOwnership::Claimed)
            }
            SubmissionClaimOutcome::Owned => Ok(SubmissionOwnership::Owned),
            SubmissionClaimOutcome::Conflict => Err(RecoveryError::Conflict),
        }
    }
}
