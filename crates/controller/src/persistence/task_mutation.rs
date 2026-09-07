use crate::domain::FailureInfo;

use super::codec::{encode_json, failure_code, failure_stage, sqlite_u64, timestamp};
use super::models::{
    CasOutcome, PublicationUpdate, TaskFailureUpdate, TaskProgressUpdate, TaskRetryUpdate,
};
use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns an error when `SQLite` access or progress encoding fails.
    pub async fn update_task_progress(
        &self,
        update: &TaskProgressUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let result = sqlx::query(
            "UPDATE tasks SET progress_json = ?, version = version + 1, updated_at_ms = ?
             WHERE id = ? AND version = ?",
        )
        .bind(encode_json("progress_json", &update.progress)?)
        .bind(timestamp(update.updated_at))
        .bind(update.task_id.to_string())
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        Ok(cas(result.rows_affected(), update.expected_version))
    }

    /// # Errors
    /// Returns an error when `SQLite` access or failure encoding fails.
    pub async fn update_task_failure(
        &self,
        update: &TaskFailureUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let (stage, code, message, retryable) = failure_bindings(update.failure.as_ref());
        let result = sqlx::query(
            "UPDATE tasks SET failure_stage = ?, failure_code = ?, failure_message = ?,
                failure_retryable = ?, version = version + 1, updated_at_ms = ?
             WHERE id = ? AND version = ?",
        )
        .bind(stage)
        .bind(code)
        .bind(message)
        .bind(retryable)
        .bind(timestamp(update.updated_at))
        .bind(update.task_id.to_string())
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        Ok(cas(result.rows_affected(), update.expected_version))
    }

    /// # Errors
    /// Returns an error when `SQLite` access or retry encoding fails.
    pub async fn update_task_retry(
        &self,
        update: &TaskRetryUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let result = sqlx::query(
            "UPDATE tasks SET retry_count = ?, next_retry_at_ms = ?,
                version = version + 1, updated_at_ms = ? WHERE id = ? AND version = ?",
        )
        .bind(i64::from(update.retry.retry_count))
        .bind(update.retry.next_retry_at.map(timestamp))
        .bind(timestamp(update.updated_at))
        .bind(update.task_id.to_string())
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        Ok(cas(result.rows_affected(), update.expected_version))
    }

    /// # Errors
    /// Returns an error when `SQLite` access or evidence encoding fails.
    pub async fn update_publication_evidence(
        &self,
        update: &PublicationUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let size = update
            .evidence
            .expected_output_size
            .map(|value| sqlite_u64("expected_output_size", value))
            .transpose()?;
        let sha = update
            .evidence
            .expected_output_sha256
            .map(|value| value.as_bytes().to_vec());
        let result = sqlx::query(
            "UPDATE tasks SET expected_output_size = ?, expected_output_sha256 = ?,
                destination_staging_name = ?, version = version + 1, updated_at_ms = ?
             WHERE id = ? AND version = ?",
        )
        .bind(size)
        .bind(sha)
        .bind(update.evidence.destination_staging_name.as_deref())
        .bind(timestamp(update.updated_at))
        .bind(update.task_id.to_string())
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        Ok(cas(result.rows_affected(), update.expected_version))
    }
}

fn failure_bindings(
    failure: Option<&FailureInfo>,
) -> (
    Option<&'static str>,
    Option<&'static str>,
    Option<&str>,
    Option<bool>,
) {
    match failure {
        Some(failure) => (
            Some(failure_stage(failure.failure_stage)),
            Some(failure_code(failure.failure_code)),
            Some(failure.message.as_str()),
            Some(failure.retryable),
        ),
        None => (None, None, None, None),
    }
}

fn cas(rows: u64, expected_version: u64) -> CasOutcome {
    if rows == 1 {
        CasOutcome::Applied {
            new_version: expected_version + 1,
        }
    } else {
        CasOutcome::Conflict
    }
}
