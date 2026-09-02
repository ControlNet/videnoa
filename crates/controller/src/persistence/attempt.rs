use sqlx::Row;

use crate::domain::{
    AttemptId, FailureInfo, RemoteJobId, RemotePath, RetryMetadata, SubmissionKey, TaskAttempt,
    TaskId, TaskProgress, WorkerId,
};

use super::codec::{
    boolean, corrupt, decode_json, parse_brand, parse_failure_code, parse_failure_stage,
    parse_optional_timestamp, parse_task_status, parse_timestamp, rust_u32, rust_u64, sqlite_u64,
    timestamp,
};
use super::models::{
    AttemptFailureUpdate, AttemptProgressUpdate, AttemptRecord, AttemptRemoteUpdate,
    AttemptTransition, CasOutcome,
};
use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn attempt(&self, id: AttemptId) -> Result<Option<AttemptRecord>, PersistenceError> {
        sqlx::query(ATTEMPT_SELECT)
            .bind(id.to_string())
            .fetch_optional(self.database.pool())
            .await?
            .as_ref()
            .map(map_attempt)
            .transpose()
    }

    /// # Errors
    /// Returns an error when `SQLite` access or persisted decoding fails.
    pub async fn task_attempts(
        &self,
        task_id: TaskId,
        limit: u16,
    ) -> Result<Vec<AttemptRecord>, PersistenceError> {
        let sql = format!("{ATTEMPT_SELECT_ALL} AND task_id = ? ORDER BY attempt_no DESC LIMIT ?");
        let rows = sqlx::query(&sql)
            .bind(task_id.to_string())
            .bind(i64::from(limit))
            .fetch_all(self.database.pool())
            .await?;
        rows.iter().map(map_attempt).collect()
    }

    /// # Errors
    /// Returns an error when `SQLite` access or value encoding fails.
    pub async fn update_attempt_remote(
        &self,
        update: &AttemptRemoteUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let result = sqlx::query(
            "UPDATE task_attempts SET remote_job_id = ?, remote_input_path = ?,
                remote_output_path = ?, submitted_at_ms = ?, updated_at_ms = ?,
                version = version + 1
             WHERE id = ? AND version = ?",
        )
        .bind(update.remote_job_id.to_string())
        .bind(update.remote_input_path.as_str())
        .bind(update.remote_output_path.as_str())
        .bind(timestamp(update.submitted_at))
        .bind(timestamp(update.submitted_at))
        .bind(update.attempt_id.to_string())
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        if result.rows_affected() == 1 {
            Ok(CasOutcome::Applied {
                new_version: update.expected_version + 1,
            })
        } else {
            Ok(CasOutcome::Conflict)
        }
    }

    /// # Errors
    /// Returns an error when `SQLite` access or value encoding fails.
    pub async fn transition_attempt(
        &self,
        transition: &AttemptTransition,
    ) -> Result<CasOutcome, PersistenceError> {
        let status = super::codec::task_status(transition.next_status);
        let occurred_at = timestamp(transition.occurred_at);
        let result = sqlx::query(
            "UPDATE task_attempts SET status = ?, version = version + 1, updated_at_ms = ?,
                started_at_ms = CASE WHEN ? = 'processing' THEN ? ELSE started_at_ms END,
                completed_at_ms = CASE WHEN ? IN ('completed', 'failed', 'cancelled')
                    THEN ? ELSE completed_at_ms END
             WHERE id = ? AND status = ? AND version = ?",
        )
        .bind(status)
        .bind(occurred_at)
        .bind(status)
        .bind(occurred_at)
        .bind(status)
        .bind(occurred_at)
        .bind(transition.attempt_id.to_string())
        .bind(super::codec::task_status(transition.expected_status))
        .bind(sqlite_u64("expected_version", transition.expected_version)?)
        .execute(self.database.pool())
        .await?;
        Ok(cas(result.rows_affected(), transition.expected_version))
    }

    /// # Errors
    /// Returns an error when `SQLite` access or value encoding fails.
    pub async fn update_attempt_progress(
        &self,
        update: &AttemptProgressUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let result = sqlx::query(
            "UPDATE task_attempts SET progress_json = ?, version = version + 1, updated_at_ms = ?
             WHERE id = ? AND version = ?",
        )
        .bind(super::codec::encode_json(
            "progress_json",
            &update.progress,
        )?)
        .bind(timestamp(update.updated_at))
        .bind(update.attempt_id.to_string())
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        Ok(cas(result.rows_affected(), update.expected_version))
    }

    /// # Errors
    /// Returns an error when `SQLite` access or value encoding fails.
    pub async fn update_attempt_failure(
        &self,
        update: &AttemptFailureUpdate,
    ) -> Result<CasOutcome, PersistenceError> {
        let (stage, code, message, retryable) = failure_bindings(update.failure.as_ref());
        let result = sqlx::query(
            "UPDATE task_attempts SET failure_stage = ?, failure_code = ?, failure_message = ?,
                failure_retryable = ?, retry_count = ?, next_retry_at_ms = ?,
                version = version + 1, updated_at_ms = ? WHERE id = ? AND version = ?",
        )
        .bind(stage)
        .bind(code)
        .bind(message)
        .bind(retryable)
        .bind(i64::from(update.retry.retry_count))
        .bind(update.retry.next_retry_at.map(timestamp))
        .bind(timestamp(update.updated_at))
        .bind(update.attempt_id.to_string())
        .bind(sqlite_u64("expected_version", update.expected_version)?)
        .execute(self.database.pool())
        .await?;
        Ok(cas(result.rows_affected(), update.expected_version))
    }
}

const ATTEMPT_SELECT: &str = "SELECT id, task_id, attempt_no, version, worker_id, status,
    submission_key, remote_job_id, remote_input_path, remote_output_path, progress_json,
    retry_count, next_retry_at_ms, failure_stage, failure_code, failure_message,
    failure_retryable, created_at_ms, updated_at_ms, started_at_ms, submitted_at_ms,
    completed_at_ms FROM task_attempts WHERE id = ?";
const ATTEMPT_SELECT_ALL: &str = "SELECT id, task_id, attempt_no, version, worker_id, status,
    submission_key, remote_job_id, remote_input_path, remote_output_path, progress_json,
    retry_count, next_retry_at_ms, failure_stage, failure_code, failure_message,
    failure_retryable, created_at_ms, updated_at_ms, started_at_ms, submitted_at_ms,
    completed_at_ms FROM task_attempts WHERE 1 = 1";

fn map_attempt(row: &sqlx::sqlite::SqliteRow) -> Result<AttemptRecord, PersistenceError> {
    let failure = map_failure(row)?;
    let worker_id = optional_brand::<WorkerId>(row, "worker_id")?;
    let remote_job_id = optional_brand::<RemoteJobId>(row, "remote_job_id")?;
    Ok(AttemptRecord {
        attempt: TaskAttempt {
            id: parse_brand::<AttemptId>("id", row.try_get("id")?)?,
            task_id: parse_brand::<TaskId>("task_id", row.try_get("task_id")?)?,
            attempt_number: rust_u32("attempt_no", row.try_get("attempt_no")?)?,
            worker_id,
            status: parse_task_status(row.try_get("status")?)?,
            submission_key: parse_brand::<SubmissionKey>(
                "submission_key",
                row.try_get("submission_key")?,
            )?,
            remote_job_id,
            remote_input_path: row
                .try_get::<Option<String>, _>("remote_input_path")?
                .map(RemotePath::new),
            remote_output_path: row
                .try_get::<Option<String>, _>("remote_output_path")?
                .map(RemotePath::new),
            progress: decode_json::<TaskProgress>("progress_json", row.try_get("progress_json")?)?,
            retry: RetryMetadata {
                retry_count: rust_u32("retry_count", row.try_get("retry_count")?)?,
                next_retry_at: parse_optional_timestamp(
                    "next_retry_at_ms",
                    row.try_get("next_retry_at_ms")?,
                )?,
            },
            failure,
            created_at: parse_timestamp("created_at_ms", row.try_get("created_at_ms")?)?,
            started_at: parse_optional_timestamp("started_at_ms", row.try_get("started_at_ms")?)?,
            completed_at: parse_optional_timestamp(
                "completed_at_ms",
                row.try_get("completed_at_ms")?,
            )?,
        },
        version: rust_u64("version", row.try_get("version")?)?,
        updated_at: parse_timestamp("updated_at_ms", row.try_get("updated_at_ms")?)?,
        submitted_at: parse_optional_timestamp("submitted_at_ms", row.try_get("submitted_at_ms")?)?,
    })
}

fn map_failure(row: &sqlx::sqlite::SqliteRow) -> Result<Option<FailureInfo>, PersistenceError> {
    let stage: Option<String> = row.try_get("failure_stage")?;
    let code: Option<String> = row.try_get("failure_code")?;
    let message: Option<String> = row.try_get("failure_message")?;
    let retryable: Option<i64> = row.try_get("failure_retryable")?;
    match (stage, code, message, retryable) {
        (None, None, None, None) => Ok(None),
        (Some(stage), Some(code), Some(message), Some(retryable)) => Ok(Some(FailureInfo {
            failure_stage: parse_failure_stage(&stage)?,
            failure_code: parse_failure_code(&code)?,
            message,
            retryable: boolean("failure_retryable", retryable)?,
        })),
        values => Err(corrupt("failure", format_args!("{values:?}"))),
    }
}

fn optional_brand<T>(
    row: &sqlx::sqlite::SqliteRow,
    field: &'static str,
) -> Result<Option<T>, PersistenceError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    row.try_get::<Option<String>, _>(field)?
        .map(|value| parse_brand(field, &value))
        .transpose()
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
            Some(super::codec::failure_stage(failure.failure_stage)),
            Some(super::codec::failure_code(failure.failure_code)),
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
