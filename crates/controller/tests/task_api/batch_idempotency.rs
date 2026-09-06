use std::fs;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use sqlx::Connection;
use tower::ServiceExt;

use super::batch_create::{options, task_count};
use super::support::{fixture, json_body, Fixture, TestResult};

fn keyed(fixture: &Fixture, body: &Value) -> TestResult<Request<Body>> {
    let mut request = fixture
        .session
        .request("POST", "/api/tasks/batch", Some(body))?;
    request
        .headers_mut()
        .insert("idempotency-key", "batch-test-key".parse()?);
    Ok(request)
}

async fn connection(fixture: &Fixture) -> TestResult<sqlx::SqliteConnection> {
    let database = fixture
        .input
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("missing workspace")?
        .join("controller.sqlite3");
    Ok(sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(database),
    )
    .await?)
}

#[tokio::test]
async fn keyed_batch_replays_original_membership_and_rejects_changed_payload() -> TestResult {
    let mut fixture = fixture().await?;
    let first = fixture
        .router
        .clone()
        .oneshot(keyed(&fixture, &options())?)
        .await?;
    assert_eq!(first.status(), StatusCode::CREATED);
    let first = json_body(first).await?;
    // Synthetic input changes must not alter an already admitted batch on replay.
    fs::remove_file(&fixture.input)?;
    fs::write(
        fixture.input.with_file_name("new.MKV"),
        b"synthetic new media",
    )?;
    super::support::restart_router(&mut fixture).await?;
    let replay = fixture
        .router
        .clone()
        .oneshot(keyed(&fixture, &options())?)
        .await?;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(json_body(replay).await?, first);
    let mut changed = options();
    changed["priority"] = json!(8);
    let conflict = fixture
        .router
        .clone()
        .oneshot(keyed(&fixture, &changed)?)
        .await?;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(task_count(&fixture).await?, 1);
    let mut connection = connection(&fixture).await?;
    let stored: String = sqlx::query_scalar("SELECT response_json FROM batch_idempotency")
        .fetch_one(&mut connection)
        .await?;
    assert_eq!(serde_json::from_str::<Value>(&stored)?, first);
    Ok(())
}

#[tokio::test]
async fn concurrent_keyed_batches_admit_exactly_one_set_of_tasks() -> TestResult {
    let fixture = fixture().await?;
    fs::copy(&fixture.input, fixture.input.with_file_name("second.MKV"))?;
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let router = fixture.router.clone();
        let request = keyed(&fixture, &options())?;
        requests.spawn(async move { router.oneshot(request).await });
    }
    let mut created = 0;
    let mut expected = None;
    while let Some(result) = requests.join_next().await {
        let response = result??;
        created += usize::from(response.status() == StatusCode::CREATED);
        assert!(matches!(
            response.status(),
            StatusCode::CREATED | StatusCode::OK
        ));
        let body = json_body(response).await?;
        if let Some(expected) = &expected {
            assert_eq!(&body, expected);
        } else {
            expected = Some(body);
        }
    }
    assert_eq!(created, 1);
    assert_eq!(task_count(&fixture).await?, 2);
    Ok(())
}

#[tokio::test]
async fn preview_rejection_does_not_consume_batch_key() -> TestResult {
    let fixture = fixture().await?;
    let occupied = fixture.output.with_file_name("source.AI.MKV");
    fs::write(&occupied, b"synthetic occupied output")?;
    let rejected = fixture
        .router
        .clone()
        .oneshot(keyed(&fixture, &options())?)
        .await?;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert_eq!(task_count(&fixture).await?, 0);
    fs::remove_file(occupied)?;
    let accepted = fixture
        .router
        .clone()
        .oneshot(keyed(&fixture, &options())?)
        .await?;
    assert_eq!(accepted.status(), StatusCode::CREATED);
    assert_eq!(task_count(&fixture).await?, 1);
    Ok(())
}

#[tokio::test]
async fn partial_batch_result_is_replayed_without_retrying_failed_items() -> TestResult {
    let fixture = fixture().await?;
    fs::copy(&fixture.input, fixture.input.with_file_name("aaa.MKV"))?;
    let mut connection = connection(&fixture).await?;
    // Test-only fault injection, matching the existing unkeyed partial-admission contract.
    sqlx::query("CREATE TRIGGER fail_keyed_task BEFORE INSERT ON tasks WHEN NEW.input_path LIKE '%source.MKV' BEGIN SELECT RAISE(ABORT, 'test-only failure'); END")
        .execute(&mut connection).await?;
    let first = fixture
        .router
        .clone()
        .oneshot(keyed(&fixture, &options())?)
        .await?;
    assert_eq!(first.status(), StatusCode::MULTI_STATUS);
    let first = json_body(first).await?;
    assert_eq!(first["created"], 1);
    assert_eq!(first["failed"], 1);
    sqlx::query("DROP TRIGGER fail_keyed_task")
        .execute(&mut connection)
        .await?;
    let replay = fixture
        .router
        .clone()
        .oneshot(keyed(&fixture, &options())?)
        .await?;
    assert_eq!(replay.status(), StatusCode::MULTI_STATUS);
    assert_eq!(json_body(replay).await?, first);
    assert_eq!(task_count(&fixture).await?, 1);
    Ok(())
}

#[tokio::test]
async fn failed_receipt_write_rolls_back_tasks_and_allows_safe_retry() -> TestResult {
    let fixture = fixture().await?;
    let mut connection = connection(&fixture).await?;
    // Test-only fault injection at the durable response checkpoint.
    sqlx::query("CREATE TRIGGER fail_receipt BEFORE UPDATE ON batch_idempotency BEGIN SELECT RAISE(ABORT, 'test-only receipt failure'); END")
        .execute(&mut connection).await?;
    let failed = fixture
        .router
        .clone()
        .oneshot(keyed(&fixture, &options())?)
        .await?;
    assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(task_count(&fixture).await?, 0);
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM batch_idempotency")
        .fetch_one(&mut connection)
        .await?;
    assert_eq!(count, 0);
    sqlx::query("DROP TRIGGER fail_receipt")
        .execute(&mut connection)
        .await?;
    let retry = fixture
        .router
        .clone()
        .oneshot(keyed(&fixture, &options())?)
        .await?;
    assert_eq!(retry.status(), StatusCode::CREATED);
    assert_eq!(task_count(&fixture).await?, 1);
    Ok(())
}

#[tokio::test]
async fn batch_keys_are_separate_from_single_tasks_and_unkeyed_requests() -> TestResult {
    let fixture = fixture().await?;
    let single_body = super::support::task_request(&fixture.input, &fixture.output, 1);
    let mut single = fixture
        .session
        .request("POST", "/api/tasks", Some(&single_body))?;
    single
        .headers_mut()
        .insert("idempotency-key", "batch-test-key".parse()?);
    assert_eq!(
        fixture.router.clone().oneshot(single).await?.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        fixture
            .router
            .clone()
            .oneshot(keyed(&fixture, &options())?)
            .await?
            .status(),
        StatusCode::CREATED
    );
    for _ in 0..2 {
        let unkeyed = fixture
            .session
            .request("POST", "/api/tasks/batch", Some(&options()))?;
        assert_eq!(
            fixture.router.clone().oneshot(unkeyed).await?.status(),
            StatusCode::CREATED
        );
    }
    assert_eq!(task_count(&fixture).await?, 4);
    Ok(())
}
