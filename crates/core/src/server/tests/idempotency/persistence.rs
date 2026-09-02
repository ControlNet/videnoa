use anyhow::Result;
use rusqlite::Connection;

use super::super::persistence::JobsPersistence;
use super::{persisted_job_count, RunFixture};

#[test]
fn legacy_jobs_table_migrates_without_synthetic_keys_or_row_loss() -> Result<()> {
    // Given: a pre-idempotency database containing one ordinary unkeyed row.
    let fixture = RunFixture::new(0)?;
    std::fs::remove_file(fixture.data_dir.join("jobs.db"))?;
    let connection = Connection::open(fixture.data_dir.join("jobs.db"))?;
    create_legacy_schema(&connection, false)?;
    insert_legacy_row(&connection, "legacy-job", None)?;
    drop(connection);

    // When: the current persistence layer initializes the old database.
    let persistence = JobsPersistence::new(&fixture.data_dir)?;
    let restored = persistence.load_jobs_for_startup()?;

    // Then: the row survives, new nullable columns exist, and no key is fabricated.
    assert_eq!(restored.len(), 1);
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 1);
    let connection = Connection::open(fixture.data_dir.join("jobs.db"))?;
    let (key, fingerprint): (Option<String>, Option<String>) = connection.query_row(
        "SELECT idempotency_key, request_fingerprint FROM jobs WHERE id = 'legacy-job'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert!(key.is_none());
    assert!(fingerprint.is_none());
    Ok(())
}

#[test]
fn migration_rejects_duplicate_existing_keys_without_corrupting_rows() -> Result<()> {
    // Given: a partially migrated database with contradictory duplicate key mappings.
    let fixture = RunFixture::new(0)?;
    std::fs::remove_file(fixture.data_dir.join("jobs.db"))?;
    let connection = Connection::open(fixture.data_dir.join("jobs.db"))?;
    create_legacy_schema(&connection, true)?;
    insert_legacy_row(&connection, "conflict-a", Some("duplicate-key"))?;
    insert_legacy_row(&connection, "conflict-b", Some("duplicate-key"))?;
    drop(connection);

    // When: initialization attempts to enforce durable key uniqueness.
    let result = JobsPersistence::new(&fixture.data_dir);

    // Then: startup reports the conflict and leaves both source rows intact.
    let error = result.expect_err("duplicate mappings must fail migration");
    assert!(error.to_string().contains("unique idempotency key"));
    assert_eq!(persisted_job_count(&fixture.data_dir)?, 2);
    Ok(())
}

fn create_legacy_schema(connection: &Connection, partial_columns: bool) -> Result<()> {
    let idempotency_columns = if partial_columns {
        ", idempotency_key TEXT, request_fingerprint TEXT"
    } else {
        ""
    };
    connection.execute_batch(&format!(
        "CREATE TABLE jobs (
            id TEXT PRIMARY KEY,
            status TEXT NOT NULL,
            workflow_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            started_at TEXT,
            completed_at TEXT,
            progress_json TEXT,
            error TEXT,
            params_json TEXT,
            workflow_name TEXT NOT NULL,
            workflow_source TEXT NOT NULL,
            rerun_of_job_id TEXT,
            updated_at TEXT NOT NULL
            {idempotency_columns}
        );"
    ))?;
    Ok(())
}

fn insert_legacy_row(connection: &Connection, id: &str, key: Option<&str>) -> Result<()> {
    let workflow = serde_json::json!({
        "nodes": [{
            "id": "delay",
            "node_type": "idempotency_delay",
            "params": {"delay_ms": 0}
        }],
        "connections": []
    });
    let has_columns = connection
        .prepare("SELECT idempotency_key FROM jobs")
        .is_ok();
    if has_columns {
        connection.execute(
            "INSERT INTO jobs (
                id, status, workflow_json, created_at, workflow_name, workflow_source,
                updated_at, idempotency_key, request_fingerprint
             ) VALUES (?1, 'queued', ?2, ?3, 'legacy', 'api_jobs', ?3, ?4, 'fingerprint')",
            rusqlite::params![
                id,
                serde_json::to_string(&workflow)?,
                chrono::Utc::now().to_rfc3339(),
                key
            ],
        )?;
    } else {
        connection.execute(
            "INSERT INTO jobs (
                id, status, workflow_json, created_at, workflow_name, workflow_source, updated_at
             ) VALUES (?1, 'queued', ?2, ?3, 'legacy', 'api_jobs', ?3)",
            rusqlite::params![
                id,
                serde_json::to_string(&workflow)?,
                chrono::Utc::now().to_rfc3339()
            ],
        )?;
    }
    Ok(())
}
