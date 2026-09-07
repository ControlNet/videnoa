use std::sync::{Arc, LazyLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::TestResult;

// One fixture already owns up to eight Tokio workers, so a 2-vCPU runner cannot
// safely admit a second process-heavy Task 20 fixture without deadline starvation.
const CONCURRENT_FIXTURE_BUDGET: usize = 1;

static FIXTURE_PERMITS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(CONCURRENT_FIXTURE_BUDGET)));

pub(super) struct FixturePermit {
    _process: ProcessFixturePermit,
    _runtime: OwnedSemaphorePermit,
}

impl FixturePermit {
    pub(super) async fn acquire() -> TestResult<Self> {
        let runtime = Arc::clone(&FIXTURE_PERMITS).acquire_owned().await?;
        let process = acquire_process_fixture_permit().await?;
        Ok(Self {
            _process: process,
            _runtime: runtime,
        })
    }
}

#[cfg(unix)]
type ProcessFixturePermit = std::fs::File;
#[cfg(not(unix))]
type ProcessFixturePermit = ();

#[cfg(unix)]
async fn acquire_process_fixture_permit() -> TestResult<ProcessFixturePermit> {
    Ok(tokio::task::spawn_blocking(|| {
        let path = std::env::temp_dir().join("videnoa-controller-task20-fixture.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive)
            .map_err(|error| std::io::Error::from_raw_os_error(error.raw_os_error()))?;
        std::io::Result::Ok(file)
    })
    .await??)
}

#[cfg(not(unix))]
async fn acquire_process_fixture_permit() -> TestResult<ProcessFixturePermit> {
    Ok(())
}
