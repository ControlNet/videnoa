use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

use super::{parse_status, parse_timestamp, status_to_str, JobsPersistence};
use crate::server::{idempotency::RequestFingerprint, CreateJobResponse, Job};

pub(crate) enum IdempotentJobClaim {
    Created,
    Replayed(CreateJobResponse),
    Conflict,
}

pub(crate) enum IdempotentJobLookup {
    Missing,
    Replayed(CreateJobResponse),
    Conflict,
}

struct PersistedIdempotencyMapping {
    id: String,
    status: String,
    created_at: String,
    fingerprint: Option<String>,
    workflow_name: String,
    params_json: Option<String>,
}

impl JobsPersistence {
    pub(crate) fn lookup_idempotent_job(
        &self,
        key: &str,
        fingerprint: &str,
    ) -> Result<IdempotentJobLookup> {
        self.with_connection(|connection| {
            let existing = Self::load_idempotency_mapping(connection, key)?;
            Self::classify_idempotency_mapping(existing, fingerprint)
        })
        .with_context(|| "failed to inspect durable idempotent job submission")
    }

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
            let existing = Self::load_idempotency_mapping(&transaction, key)?;

            let outcome = match Self::classify_idempotency_mapping(existing, fingerprint)? {
                IdempotentJobLookup::Missing => {
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
                IdempotentJobLookup::Replayed(existing) => IdempotentJobClaim::Replayed(existing),
                IdempotentJobLookup::Conflict => IdempotentJobClaim::Conflict,
            };
            transaction.commit()?;
            Ok(outcome)
        })
        .with_context(|| "failed to claim durable idempotent job submission")
    }

    fn load_idempotency_mapping(
        connection: &rusqlite::Connection,
        key: &str,
    ) -> rusqlite::Result<Option<PersistedIdempotencyMapping>> {
        connection
            .query_row(
                "SELECT id, status, created_at, request_fingerprint, workflow_name, params_json
                 FROM jobs WHERE idempotency_key = ?1",
                params![key],
                |row| {
                    Ok(PersistedIdempotencyMapping {
                        id: row.get(0)?,
                        status: row.get(1)?,
                        created_at: row.get(2)?,
                        fingerprint: row.get(3)?,
                        workflow_name: row.get(4)?,
                        params_json: row.get(5)?,
                    })
                },
            )
            .optional()
    }

    fn classify_idempotency_mapping(
        existing: Option<PersistedIdempotencyMapping>,
        fingerprint: &str,
    ) -> Result<IdempotentJobLookup> {
        let Some(existing) = existing else {
            return Ok(IdempotentJobLookup::Missing);
        };
        let stored_fingerprint = existing
            .fingerprint
            .ok_or_else(|| anyhow!("persisted idempotency mapping is missing its fingerprint"))?;
        let fingerprint_matches = if stored_fingerprint == fingerprint {
            true
        } else {
            let persisted_params = existing
                .params_json
                .as_deref()
                .map(serde_json::from_str::<HashMap<String, serde_json::Value>>)
                .transpose()
                .with_context(|| "persisted idempotency job has invalid params")?;
            RequestFingerprint::for_run(&existing.workflow_name, persisted_params.as_ref())?
                .as_str()
                == fingerprint
        };
        if !fingerprint_matches {
            return Ok(IdempotentJobLookup::Conflict);
        }
        let status = parse_status(&existing.status)
            .ok_or_else(|| anyhow!("persisted idempotency job has invalid status"))?;
        Ok(IdempotentJobLookup::Replayed(CreateJobResponse {
            id: existing.id,
            status,
            created_at: parse_timestamp(&existing.created_at)?,
        }))
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
