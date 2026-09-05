use crate::lifecycle::{CancellationWrite, FailureWrite, PairedTransition, TransitionEvidence};

use super::codec::{failure_code, failure_stage, sqlite_u64, task_status, timestamp};
use super::lifecycle_evidence::{bind_submission, bind_upload};
use super::lifecycle_status::{update_attempt_status, update_task_status};
use super::{CasOutcome, PersistenceError, Store};

impl Store {
    pub(crate) async fn apply_lifecycle_transition(
        &self,
        write: &PairedTransition,
    ) -> Result<CasOutcome, PersistenceError> {
        // Submit's caller retains admission through remote acceptance. Upload intake
        // acquires it here; downstream transitions remain available while paused.
        let _admission = if write.to == crate::domain::TaskStatus::Uploading {
            Some(self.submission_admission().read_owned().await)
        } else {
            None
        };
        let paused = self.config_manager().scheduler().paused;
        let mut transaction = self.database.pool().begin().await?;
        if update_task_status(&mut transaction, write, paused).await? != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        let attempt_rows = match &write.evidence {
            TransitionEvidence::None
            | TransitionEvidence::Download(_)
            | TransitionEvidence::Publication(_) => {
                update_attempt_status(&mut transaction, write).await?
            }
            TransitionEvidence::Upload(evidence) => {
                bind_upload(&mut transaction, write, evidence).await?
            }
            TransitionEvidence::Submission(evidence) => {
                bind_submission(&mut transaction, write, evidence).await?
            }
        };
        if attempt_rows != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        transaction.commit().await?;
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }

    pub(crate) async fn fail_lifecycle(
        &self,
        write: &FailureWrite,
    ) -> Result<CasOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        let occurred_at = timestamp(write.occurred_at);
        let result = sqlx::query(
            "UPDATE tasks SET status = 'failed', failure_stage = ?, failure_code = ?,
                failure_message = ?, failure_retryable = ?, next_retry_at_ms = NULL,
                version = version + 1, updated_at_ms = ?
             WHERE id = ? AND status = ? AND version = ?",
        )
        .bind(failure_stage(write.failure.failure_stage))
        .bind(failure_code(write.failure.failure_code))
        .bind(write.failure.message.as_str())
        .bind(write.failure.retryable)
        .bind(occurred_at)
        .bind(write.task_id.to_string())
        .bind(task_status(write.from))
        .bind(sqlite_u64("task_version", write.task_version)?)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        if let Some(attempt) = write.attempt {
            let result = sqlx::query(
                "UPDATE task_attempts SET status = 'failed', failure_stage = ?, failure_code = ?,
                    failure_message = ?, failure_retryable = ?, next_retry_at_ms = NULL,
                    completed_at_ms = ?, version = version + 1, updated_at_ms = ?
                 WHERE id = ? AND status = ? AND version = ?",
            )
            .bind(failure_stage(write.failure.failure_stage))
            .bind(failure_code(write.failure.failure_code))
            .bind(write.failure.message.as_str())
            .bind(write.failure.retryable)
            .bind(occurred_at)
            .bind(occurred_at)
            .bind(attempt.id.to_string())
            .bind(task_status(attempt.status))
            .bind(sqlite_u64("attempt_version", attempt.version)?)
            .execute(&mut *transaction)
            .await?;
            if result.rows_affected() != 1 {
                transaction.rollback().await?;
                return Ok(CasOutcome::Conflict);
            }
        }
        transaction.commit().await?;
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }

    pub(crate) async fn request_lifecycle_cancellation(
        &self,
        write: &CancellationWrite,
    ) -> Result<CasOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        let requested_at = timestamp(write.requested_at);
        let next_status = if write.immediate {
            "cancelled"
        } else {
            task_status(write.from)
        };
        let result = sqlx::query(
            "UPDATE tasks SET status = ?, cancel_requested_at_ms = ?, version = version + 1,
                updated_at_ms = ? WHERE id = ? AND status = ? AND version = ?
                AND cancel_requested_at_ms IS NULL",
        )
        .bind(next_status)
        .bind(requested_at)
        .bind(requested_at)
        .bind(write.task_id.to_string())
        .bind(task_status(write.from))
        .bind(sqlite_u64("task_version", write.task_version)?)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(CasOutcome::Conflict);
        }
        if write.immediate {
            if let Some(attempt) = write.attempt {
                let result = sqlx::query(
                    "UPDATE task_attempts SET status = 'cancelled', completed_at_ms = ?,
                        version = version + 1, updated_at_ms = ?
                     WHERE id = ? AND status = ? AND version = ?",
                )
                .bind(requested_at)
                .bind(requested_at)
                .bind(attempt.id.to_string())
                .bind(task_status(attempt.status))
                .bind(sqlite_u64("attempt_version", attempt.version)?)
                .execute(&mut *transaction)
                .await?;
                if result.rows_affected() != 1 {
                    transaction.rollback().await?;
                    return Ok(CasOutcome::Conflict);
                }
            }
        }
        transaction.commit().await?;
        Ok(CasOutcome::Applied {
            new_version: write.task_version + 1,
        })
    }
}
