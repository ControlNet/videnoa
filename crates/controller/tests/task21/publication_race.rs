use std::sync::Arc;

use tokio::sync::Barrier;
use tokio::task::JoinSet;
use videnoa_controller::domain::TaskStatus;
use videnoa_controller::scheduler::PublicationOutcome;

use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{assert_status, output_path, verified_task};
use crate::transfer_support::TestResult;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn duplicate_publication_finalizers_preserve_exactly_one_final_artifact() -> TestResult {
    // Given: two production finalizers for one verified task share a deterministic start barrier.
    let server = MockVidenoa::start().await?;
    let expected = b"task-21-duplicate-finalizer".repeat(1024);
    let (fixture, prepared) = verified_task(&server, &expected).await?;
    let barrier = Arc::new(Barrier::new(3));
    let mut finalizers = JoinSet::new();
    for _ in 0..2 {
        let barrier = Arc::clone(&barrier);
        let executor = fixture.executor()?;
        let task_id = prepared.task_id;
        let now = fixture.now;
        finalizers.spawn(async move {
            barrier.wait().await;
            executor
                .publish(
                    task_id,
                    now,
                    videnoa_controller::lifecycle::JitterSample::try_from(0)?,
                )
                .await
        });
    }

    // When: both finalizers are released together and every durable result settles.
    barrier.wait().await;
    let mut completed = 0;
    let mut settled = 0;
    while let Some(result) = finalizers.join_next().await {
        settled += 1;
        if matches!(result?, Ok(PublicationOutcome::Completed)) {
            completed += 1;
        }
    }

    // Then: one finalizer completes, the duplicate cannot clobber bytes, and lifecycle is durable.
    assert_eq!(settled, 2);
    assert_eq!(completed, 1);
    assert_eq!(
        tokio::fs::read(output_path(&fixture, &prepared).await?).await?,
        expected
    );
    assert_status(&fixture, &prepared, TaskStatus::Completed).await?;
    eprintln!(
        "task21_publication contenders=2 completed=1 final_bytes={}",
        expected.len()
    );
    Ok(())
}
