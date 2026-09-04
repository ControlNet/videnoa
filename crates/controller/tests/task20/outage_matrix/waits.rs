use std::time::Duration;

use videnoa_controller::domain::WorkerId;

use crate::mock_videnoa::journal::Route;
use crate::mock_videnoa::server::MockVidenoa;
use crate::support::{ControllerFixture, TestResult};

pub(super) async fn wait_for_run_journal_entries(
    worker: &MockVidenoa,
    expected: usize,
) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let entries = worker
                .journal()
                .await
                .into_iter()
                .filter(|entry| entry.route == Route::Run)
                .count();
            if entries >= expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("submission journal did not converge"))?;
    Ok(())
}

pub(super) async fn wait_for_run_requests(worker: &MockVidenoa, expected: u64) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        while worker.counters().await.get(Route::Run) < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("submission replay was not observed after restart"))?;
    Ok(())
}

pub(super) async fn wait_for_remote_job(worker: &MockVidenoa) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        while worker.job_count().await == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("remote job was not created"))?;
    Ok(())
}

pub(super) async fn wait_for_worker_offline(
    fixture: &ControllerFixture,
    worker_id: WorkerId,
) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if fixture
                .store
                .worker(worker_id)
                .await?
                .is_some_and(|worker| !worker.online)
            {
                return Ok::<_, videnoa_controller::persistence::PersistenceError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| std::io::Error::other("worker was not marked offline"))??;
    Ok(())
}
