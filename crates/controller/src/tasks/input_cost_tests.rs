use std::io::Read;

use serde_json::json;
use tempfile::TempDir;

use crate::config::ControllerConfig;
use crate::domain::{IdempotencyKey, TaskCreateRequest};
use crate::paths::{input_hash_counts::count, PathCapabilities};
use crate::persistence::{Database, DatabaseOptions, Store};
use crate::scheduler::upload_input::open_verified;

use super::intake::{IntakeOutcome, TaskService};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

async fn fixture() -> Result<(TempDir, TaskService, Store), Box<dyn std::error::Error + Send + Sync>>
{
    let directory = TempDir::new()?;
    let config = ControllerConfig::for_workspace(directory.path())?;
    std::fs::create_dir(directory.path().join("data"))?;
    let paths = PathCapabilities::open(&config.paths)?;
    let store = Store::new(
        Database::open(DatabaseOptions::new(
            directory.path().join("data/controller.sqlite3"),
        ))
        .await?,
    );
    Ok((directory, TaskService::new(store.clone(), paths), store))
}

#[tokio::test]
async fn intake_and_upload_each_hash_once_and_transfer_starts_at_byte_zero() -> TestResult {
    let (directory, service, store) = fixture().await?;
    let input = directory.path().join("input.mkv");
    // Synthetic test-only content spans multiple hashing buffers.
    let bytes = vec![42_u8; 200_000];
    std::fs::write(&input, &bytes)?;
    let request: TaskCreateRequest = serde_json::from_value(json!({
        "input_path": input, "output_path": directory.path().join("output.mp4"),
        "workflow": "test-only-workflow", "priority": 0, "source": "api", "source_reference": null
    }))?;
    let result = service
        .create(IdempotencyKey::new("test-only-hash-count"), request)
        .await
        .map_err(|_| std::io::Error::other("task creation failed"))?;
    let IntakeOutcome::Created(task) = result else {
        return Err("task was not created".into());
    };
    assert_eq!(count(&input), 1, "intake must hash only once");
    let record = store.task(task.id).await?.ok_or("task missing")?;
    let mut file = open_verified(&service.paths, &record)
        .map_err(|_| std::io::Error::other("upload verification failed"))?;
    assert_eq!(
        count(&input),
        2,
        "upload admission must add exactly one hash"
    );
    let mut uploaded = Vec::new();
    file.read_to_end(&mut uploaded)?;
    assert_eq!(uploaded, bytes);
    assert_eq!(count(&input), 2, "actual transfer must not rehash");
    Ok(())
}

#[tokio::test]
async fn batch_preview_hashes_nothing_and_creation_hashes_each_file_once() -> TestResult {
    let (directory, service, _) = fixture().await?;
    let inputs = [
        directory.path().join("one.mkv"),
        directory.path().join("two.mkv"),
    ];
    for input in &inputs {
        std::fs::write(input, b"test-only media")?;
    }
    let request = json!({
        "input_pattern": directory.path().join("*.mkv"), "output_mode": "beside_input",
        "output_directory": null, "naming_mode": "insert_extension", "middle_extension": "AI",
        "workflow": "test-only-workflow", "priority": 0
    });
    service
        .preview_batch(serde_json::from_value(request.clone())?)
        .await
        .map_err(|_| std::io::Error::other("preview failed"))?;
    for input in &inputs {
        assert_eq!(count(input), 0);
    }
    let (status, response) = service
        .create_batch(serde_json::from_value(request)?)
        .await
        .map_err(|_| std::io::Error::other("batch creation failed"))?;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    assert_eq!(response.created, 2);
    for input in &inputs {
        assert_eq!(count(input), 1);
    }
    Ok(())
}
