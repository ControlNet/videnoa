use anyhow::{anyhow, Context, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

use super::{parse_status, parse_timestamp, status_to_str, JobsPersistence};
use crate::server::{CreateJobResponse, Job};

pub(crate) enum IdempotentJobClaim {
    Created,
    Replayed(CreateJobResponse),
    Conflict,
}

impl JobsPersistence {
    pub(crate) fn claim_idempotent_job(
        &self,
        key: &str,
        fingerprint: &str,
        job: &Job,
    ) -> Result<IdempotentJobClaim> {
        let row = Self::row_from_job(job)?;
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = transaction
                .query_row(
                    "SELECT id, status, created_at, request_fingerprint
                     FROM jobs WHERE idempotency_key = ?1",
                    params![key],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                        ))
                    },
                )
                .optional()?;

            let outcome = match existing {
                Some((id, status, created_at, stored_fingerprint)) => {
                    let stored_fingerprint = stored_fingerprint.ok_or_else(|| {
                        anyhow!("persisted idempotency mapping is missing its fingerprint")
                    })?;
                    if stored_fingerprint != fingerprint {
                        IdempotentJobClaim::Conflict
                    } else {
                        let status = parse_status(&status).ok_or_else(|| {
                            anyhow!("persisted idempotency job has invalid status")
                        })?;
                        IdempotentJobClaim::Replayed(CreateJobResponse {
                            id,
                            status,
                            created_at: parse_timestamp(&created_at)?,
                        })
                    }
                }
                None => {
                    transaction.execute(
                        "INSERT INTO jobs (
                            id, status, workflow_json, created_at, started_at, completed_at,
                            progress_json, error, params_json, workflow_name, workflow_source,
                            rerun_of_job_id, updated_at, idempotency_key, request_fingerprint
                         ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15
                         )",
                        params![
                            row.id,
                            status_to_str(row.status),
                            row.workflow_json,
                            row.created_at.to_rfc3339(),
                            row.started_at.map(|value| value.to_rfc3339()),
                            row.completed_at.map(|value| value.to_rfc3339()),
                            row.progress_json,
                            row.error,
                            row.params_json,
                            row.workflow_name,
                            row.workflow_source,
                            row.rerun_of_job_id,
                            chrono::Utc::now().to_rfc3339(),
                            key,
                            fingerprint,
                        ],
                    )?;
                    IdempotentJobClaim::Created
                }
            };
            transaction.commit()?;
            Ok(outcome)
        })
        .with_context(|| "failed to claim durable idempotent job submission")
    }

    pub(super) fn migrate_idempotency_schema(connection: &mut rusqlite::Connection) -> Result<()> {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement = transaction.prepare("PRAGMA table_info(jobs)")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        if !columns.iter().any(|column| column == "idempotency_key") {
            transaction.execute("ALTER TABLE jobs ADD COLUMN idempotency_key TEXT", [])?;
        }
        if !columns.iter().any(|column| column == "request_fingerprint") {
            transaction.execute("ALTER TABLE jobs ADD COLUMN request_fingerprint TEXT", [])?;
        }

        let incomplete: u64 = transaction.query_row(
            "SELECT COUNT(*) FROM jobs
             WHERE (idempotency_key IS NULL) <> (request_fingerprint IS NULL)",
            [],
            |row| row.get(0),
        )?;
        if incomplete > 0 {
            return Err(anyhow!("incomplete persisted idempotency mapping"));
        }

        transaction
            .execute(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_idempotency_key
                 ON jobs(idempotency_key) WHERE idempotency_key IS NOT NULL",
                [],
            )
            .with_context(|| "failed to enforce unique idempotency key")?;
        transaction.commit()?;
        Ok(())
    }
}
