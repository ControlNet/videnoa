//! SQL spellings of the task status groups defined on [`TaskStatus`], so every
//! query counts occupancy with the same statuses as the lifecycle.

use std::sync::LazyLock;

use crate::domain::TaskStatus;

use super::codec::task_status;

/// `IN` list of the statuses that hold a compute slot.
pub(super) static COMPUTE: LazyLock<String> =
    LazyLock::new(|| status_list(TaskStatus::occupies_compute));

/// `IN` list of the statuses assigned to a Worker ahead of compute.
pub(super) static STAGE_IN: LazyLock<String> =
    LazyLock::new(|| status_list(TaskStatus::is_stage_in));

/// `IN` list of the terminal statuses. The partial recovery indexes repeat this
/// spelling, and SQLite uses them only when a query spells it the same way.
pub(super) static TERMINAL: LazyLock<String> =
    LazyLock::new(|| status_list(TaskStatus::is_terminal));

fn status_list(member: fn(TaskStatus) -> bool) -> String {
    let statuses = TaskStatus::ALL
        .iter()
        .copied()
        .filter(|status| member(*status))
        .map(|status| format!("'{}'", task_status(status)))
        .collect::<Vec<_>>();
    format!("({})", statuses.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_groups_keep_the_spelling_of_the_schema() {
        assert_eq!(*COMPUTE, "('submitting', 'processing')");
        assert_eq!(*STAGE_IN, "('reserved', 'uploading', 'staged')");
        // Must match `idx_tasks_recovery` and `idx_attempts_recovery` in 0001.
        assert_eq!(*TERMINAL, "('completed', 'failed', 'cancelled')");
    }

    /// The schema repeats the status and failure vocabulary in CHECK
    /// constraints, which a migration cannot extend in place: a new value must
    /// come with a table rebuild, or this test fails.
    #[tokio::test]
    async fn schema_checks_accept_every_status_and_failure_value() {
        use super::super::codec::{failure_code, failure_stage};
        use crate::domain::{FailureCode, FailureStage};

        let directory = tempfile::TempDir::new().expect("temp directory");
        let database = crate::persistence::Database::open(
            crate::persistence::DatabaseOptions::new(directory.path().join("controller.sqlite3")),
        )
        .await
        .expect("database");
        let pool = database.pool();
        let rows = TaskStatus::ALL
            .len()
            .max(FailureCode::ALL.len())
            .max(FailureStage::ALL.len());
        for index in 0..rows {
            let status = TaskStatus::ALL[index % TaskStatus::ALL.len()];
            let code = FailureCode::ALL[index % FailureCode::ALL.len()];
            let stage = &FailureStage::ALL[index % FailureStage::ALL.len()];
            assert_eq!(
                serde_json::to_value(status).expect("status"),
                task_status(status)
            );
            let task_id = format!("00000000-0000-4000-8000-{index:012}");
            sqlx::query(
                "INSERT INTO tasks (id, status, input_path, output_path, input_extension,
                    output_extension, workflow, priority, source, input_size, input_mtime_ms,
                    progress_json, failure_stage, failure_code, failure_message,
                    failure_retryable, created_at_ms, updated_at_ms)
                 VALUES (?, ?, '/in.mkv', '/out.mp4', 'mkv', 'mp4', 'w', 0, 'api', 1, 1,
                    '{\"percent\":0}', ?, ?, 'message', 0, 1, 1)",
            )
            .bind(&task_id)
            .bind(task_status(status))
            .bind(failure_stage(*stage))
            .bind(failure_code(code))
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("{status:?}/{code:?}/{stage:?}: {error}"));
            sqlx::query(
                "INSERT INTO task_attempts (id, task_id, attempt_no, status, submission_key,
                    progress_json, failure_stage, failure_code, failure_message,
                    failure_retryable, created_at_ms, updated_at_ms)
                 VALUES (?, ?, 1, ?, ?, '{\"percent\":0}', ?, ?, 'message', 0, 1, 1)",
            )
            .bind(format!("10000000-0000-4000-8000-{index:012}"))
            .bind(&task_id)
            .bind(task_status(status))
            .bind(format!("20000000-0000-4000-8000-{index:012}"))
            .bind(failure_stage(*stage))
            .bind(failure_code(code))
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("{status:?}/{code:?}/{stage:?}: {error}"));
        }
        database.close().await;
    }
}
