use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use videnoa_controller::domain::{AttemptId, FailureCode, TaskId, TaskStatus};
use videnoa_controller::scheduler::DownloadOutcome;

use crate::mock_videnoa::faults::OfflineMode;
use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{part_path, verified_path, zero_jitter, Fixture, TestResult};

#[tokio::test]
async fn restart_reuses_matching_verified_artifact_after_rename_before_cas() -> TestResult {
    // Given: a production download installed verified bytes before its lifecycle CAS was retained.
    let mut server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![29_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;
    restore_downloading(&fixture, prepared.task_id, prepared.attempt_id).await?;
    let verified = verified_path(&fixture.temp_root, prepared.task_id);
    server.set_offline(OfflineMode::ConnectionRefused).await?;
    let stat_before = server.counters().await.get(Route::Stat);
    let get_before = server.counters().await.get(Route::Download);

    // When: restart repeats download and reaches the existing verified path.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: identical evidence is preserved and the lifecycle CAS completes.
    assert!(matches!(outcome, DownloadOutcome::Verified(_)));
    assert_eq!(tokio::fs::read(&verified).await?, output);
    assert!(!part_path(&fixture.temp_root, prepared.task_id).exists());
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Verifying
    );
    assert_eq!(server.counters().await.get(Route::Stat), stat_before);
    assert_eq!(server.counters().await.get(Route::Download), get_before);
    Ok(())
}

#[tokio::test]
async fn restart_replaces_mismatching_verified_artifact_before_cas() -> TestResult {
    // Given: a production verified artifact is corrupted before its lifecycle CAS is retained.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![31_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;
    restore_downloading(&fixture, prepared.task_id, prepared.attempt_id).await?;
    let verified = verified_path(&fixture.temp_root, prepared.task_id);
    tokio::fs::write(&verified, b"stale verified bytes").await?;
    let get_before = server.counters().await.get(Route::Download);

    // When: restart installs the newly downloaded verified artifact.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: the fixed path contains only the exact current remote output.
    assert!(matches!(outcome, DownloadOutcome::Verified(_)));
    assert_eq!(tokio::fs::read(&verified).await?, output);
    assert!(!part_path(&fixture.temp_root, prepared.task_id).exists());
    assert_eq!(server.counters().await.get(Route::Download), get_before + 1);
    assert_eq!(
        fixture.task(prepared.task_id).await?.status,
        TaskStatus::Verifying
    );
    Ok(())
}

#[tokio::test]
async fn missing_remote_job_evidence_fails_before_download_network_access() -> TestResult {
    // Given: a remote-completed attempt whose durable job identity is missing.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![53_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    update_attempt(
        &fixture,
        prepared.attempt_id,
        "UPDATE task_attempts SET remote_job_id = NULL WHERE id = ?",
    )
    .await?;
    let stat_before = server.counters().await.get(Route::Stat);
    let get_before = server.counters().await.get(Route::Download);

    // When: download admission validates the paired remote evidence.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: ambiguity is terminal before stat or GET can observe another output.
    assert!(matches!(outcome, DownloadOutcome::Failed));
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.map(|failure| failure.failure_code),
        Some(FailureCode::RemoteStateAmbiguous)
    );
    assert_eq!(server.counters().await.get(Route::Stat), stat_before);
    assert_eq!(server.counters().await.get(Route::Download), get_before);
    Ok(())
}

#[tokio::test]
async fn contradictory_remote_output_evidence_fails_before_download_network_access() -> TestResult {
    // Given: a remote-completed attempt whose output is not the input's exact opaque sibling.
    let server = MockVidenoa::start().await?;
    let fixture = Fixture::new(&server, 1, 1).await?;
    let output = vec![59_u8; 20_000];
    let prepared = fixture.remote_completed(&server, &output).await?;
    update_attempt(
        &fixture,
        prepared.attempt_id,
        "UPDATE task_attempts SET remote_output_path = 'other/output.mp4' WHERE id = ?",
    )
    .await?;
    let stat_before = server.counters().await.get(Route::Stat);
    let get_before = server.counters().await.get(Route::Download);

    // When: download admission validates exact durable workflow-path evidence.
    let outcome = fixture
        .executor()?
        .download(prepared.task_id, fixture.now, zero_jitter()?)
        .await?;

    // Then: ambiguity closes both rows without stat or GET.
    assert!(matches!(outcome, DownloadOutcome::Failed));
    let task = fixture.task(prepared.task_id).await?;
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(
        task.failure.map(|failure| failure.failure_code),
        Some(FailureCode::RemoteStateAmbiguous)
    );
    assert_eq!(server.counters().await.get(Route::Stat), stat_before);
    assert_eq!(server.counters().await.get(Route::Download), get_before);
    Ok(())
}

async fn update_attempt(fixture: &Fixture, attempt_id: AttemptId, sql: &str) -> TestResult {
    let options =
        SqliteConnectOptions::new().filename(fixture.directory.path().join("controller.sqlite3"));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    sqlx::query(sql)
        .bind(attempt_id.to_string())
        .execute(&pool)
        .await?;
    pool.close().await;
    Ok(())
}

async fn restore_downloading(
    fixture: &Fixture,
    task_id: TaskId,
    attempt_id: AttemptId,
) -> TestResult {
    let options =
        SqliteConnectOptions::new().filename(fixture.directory.path().join("controller.sqlite3"));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    sqlx::query(
        "UPDATE tasks SET status = 'downloading', expected_output_size = NULL,
            expected_output_sha256 = NULL WHERE id = ?",
    )
    .bind(task_id.to_string())
    .execute(&pool)
    .await?;
    sqlx::query("UPDATE task_attempts SET status = 'downloading' WHERE id = ?")
        .bind(attempt_id.to_string())
        .execute(&pool)
        .await?;
    pool.close().await;
    Ok(())
}
