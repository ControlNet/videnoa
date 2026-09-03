use std::collections::BTreeMap;

use crate::mock_videnoa::checkpoints::Checkpoint;
use crate::mock_videnoa::journal::{
    sanitize_entries, HeaderValueSnapshot, JournalEntry, JournalHeader, JournalOutcome, Route,
};
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{assert_completed_pipeline, complete_mock_job, ControllerFixture, TestResult};

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn three_worker_real_http_pipeline_uses_all_capacity_without_duplicates() -> TestResult {
    // Given: three one-slot persistent workers registered through Controller HTTP.
    let workers = [
        MockVidenoa::start_persistent().await?,
        MockVidenoa::start_persistent().await?,
        MockVidenoa::start_persistent().await?,
    ];
    let fixture = ControllerFixture::start().await?;
    let mut registered = Vec::new();
    for (index, worker) in workers.iter().enumerate() {
        registered.push(
            fixture
                .register_worker(worker, &format!("worker-{index}"))
                .await?,
        );
    }
    let run_a = workers[0]
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let run_b = workers[1]
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;
    let run_c = workers[2]
        .pause(Checkpoint::AfterRunPersistedBeforeResponse)
        .await;

    // When: three independent tasks enter through the authenticated HTTP boundary.
    let tasks = [
        fixture.create_task("multi-a", b"input-a").await?,
        fixture.create_task("multi-b", b"input-b").await?,
        fixture.create_task("multi-c", b"input-c").await?,
    ];
    let mut queued = 0_usize;
    for task in &tasks {
        queued += usize::from(
            fixture.task(task).await?.task.status == videnoa_controller::domain::TaskStatus::Queued,
        );
    }
    eprintln!(
        "task20_multi_pre_dispatch queued={queued} run_requests=[{},{},{}] remote_jobs=[{},{},{}]",
        workers[0]
            .counters()
            .await
            .get(crate::mock_videnoa::journal::Route::Run),
        workers[1]
            .counters()
            .await
            .get(crate::mock_videnoa::journal::Route::Run),
        workers[2]
            .counters()
            .await
            .get(crate::mock_videnoa::journal::Route::Run),
        workers[0].job_count().await,
        workers[1].job_count().await,
        workers[2].job_count().await,
    );

    // Then: all three workers must receive one keyed run without slot leakage.
    tokio::try_join!(
        workers[0].await_checkpoint(&run_a),
        workers[1].await_checkpoint(&run_b),
        workers[2].await_checkpoint(&run_c),
    )
    .map_err(|error| {
        std::io::Error::other(format!(
            "Controller did not consume all three eligible worker slots: {error}"
        ))
    })?;
    for worker in &registered {
        assert_eq!(fixture.store.worker_used_slots(worker.id).await?, 1);
    }
    let mut assigned_workers = Vec::new();
    for task in &tasks {
        let worker_id = fixture
            .task(task)
            .await?
            .task
            .worker_id
            .ok_or_else(|| std::io::Error::other("dispatched task has no worker"))?;
        let worker_index = registered
            .iter()
            .position(|worker| worker.id == worker_id)
            .ok_or_else(|| std::io::Error::other("task references an unknown worker"))?;
        assigned_workers.push(worker_index);
        complete_mock_job(&workers[worker_index], task, task.id.to_string().as_bytes()).await?;
    }
    workers[0].release(run_a).await?;
    workers[1].release(run_b).await?;
    workers[2].release(run_c).await?;
    for (task, worker_index) in tasks.iter().zip(assigned_workers) {
        assert_completed_pipeline(
            &fixture,
            &workers[worker_index],
            task,
            task.id.to_string().as_bytes(),
        )
        .await?;
    }
    for worker in &registered {
        assert_eq!(fixture.store.worker_used_slots(worker.id).await?, 0);
    }
    write_evidence(&workers).await?;
    Ok(())
}

async fn write_evidence(workers: &[MockVidenoa; 3]) -> TestResult {
    let Some(directory) = std::env::var_os("VIDENOA_TASK20_MULTI_EVIDENCE") else {
        return Ok(());
    };
    for (index, worker) in workers.iter().enumerate() {
        worker
            .write_journal(
                std::path::Path::new(&directory)
                    .join(format!("request-journal-worker-{}.json", index + 1)),
            )
            .await?;
    }
    Ok(())
}

#[test]
fn evidence_sanitization_redacts_volatile_request_identity() {
    // Given: a captured request containing one task UUID in its path/body and submission key.
    let identifier = "05f8f158-a8f9-4cdd-be98-1286b1c6726f";
    let entries = [JournalEntry {
        sequence: 1,
        method: "POST".to_owned(),
        path: format!("/api/files/{identifier}/input.mkv"),
        headers: vec![JournalHeader {
            name: "idempotency-key".to_owned(),
            value: HeaderValueSnapshot::Bytes(identifier.as_bytes().to_vec()),
        }],
        body: format!(r#"{{"input":"../mock-worker/workspace/{identifier}/input.mkv"}}"#)
            .into_bytes(),
        response_status: 200,
        route: Route::Run,
        checkpoints: BTreeMap::default(),
        outcome: JournalOutcome::Delivered,
    }];

    // When: the captured journal is prepared for persisted evidence.
    let sanitized = sanitize_entries(&entries);

    // Then: volatile identity is absent while the request shape remains inspectable.
    assert_eq!(sanitized[0].path, "/api/files/{id}/input.mkv");
    assert_eq!(sanitized[0].headers[0].value, HeaderValueSnapshot::Redacted);
    assert!(!String::from_utf8_lossy(&sanitized[0].body).contains(identifier));
}
