use sqlx::Row;

use super::codec::{encode_json, sqlite_u64, timestamp};
use super::models::{empty_progress, Reservation, ReservationOutcome};
use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns an error when the atomic `SQLite` reservation cannot complete.
    pub async fn reserve_task(
        &self,
        reservation: &Reservation,
    ) -> Result<ReservationOutcome, PersistenceError> {
        let mut transaction = self.database.pool().begin().await?;
        let attempt_no = sqlx::query(
            "UPDATE tasks SET status = 'reserved', worker_id = ?, version = version + 1,
                attempt_count = attempt_count + 1, updated_at_ms = ?, reserved_at_ms = ?
             WHERE id = ? AND status = 'queued' AND version = ?
               AND EXISTS (
                   SELECT 1 FROM workers
                   WHERE id = ? AND enabled = 1 AND online = 1
               )
               AND (
                   SELECT COUNT(*) FROM tasks assigned
                   WHERE assigned.worker_id = ?
                     AND assigned.status NOT IN ('completed', 'failed', 'cancelled')
               ) < (
                   SELECT compute_slots FROM workers WHERE id = ?
               )
             RETURNING attempt_count",
        )
        .bind(reservation.worker_id.to_string())
        .bind(timestamp(reservation.reserved_at))
        .bind(timestamp(reservation.reserved_at))
        .bind(reservation.task_id.to_string())
        .bind(sqlite_u64(
            "expected_task_version",
            reservation.expected_task_version,
        )?)
        .bind(reservation.worker_id.to_string())
        .bind(reservation.worker_id.to_string())
        .bind(reservation.worker_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(attempt_no) = attempt_no else {
            transaction.rollback().await?;
            return Ok(ReservationOutcome::Conflict);
        };
        let attempt_no: i64 = attempt_no.try_get("attempt_count")?;
        sqlx::query(
            "INSERT INTO task_attempts (
                id, task_id, attempt_no, worker_id, status, submission_key,
                progress_json, created_at_ms, updated_at_ms
             ) VALUES (?, ?, ?, ?, 'reserved', ?, ?, ?, ?)",
        )
        .bind(reservation.attempt_id.to_string())
        .bind(reservation.task_id.to_string())
        .bind(attempt_no)
        .bind(reservation.worker_id.to_string())
        .bind(reservation.submission_key.to_string())
        .bind(encode_json("progress_json", &empty_progress())?)
        .bind(timestamp(reservation.reserved_at))
        .bind(timestamp(reservation.reserved_at))
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE workers SET last_assigned_at_ms = ?, updated_at_ms = ? WHERE id = ?")
            .bind(timestamp(reservation.reserved_at))
            .bind(timestamp(reservation.reserved_at))
            .bind(reservation.worker_id.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(ReservationOutcome::Reserved(reservation.attempt_id))
    }
}
