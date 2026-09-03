use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use tokio::sync::Semaphore;
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

use super::TestResult;

pub struct CheckpointGate {
    point: TransferCheckpointPoint,
    reached: Arc<Semaphore>,
    released: Arc<Semaphore>,
    triggered: AtomicBool,
}

impl CheckpointGate {
    pub fn new(point: TransferCheckpointPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            reached: Arc::new(Semaphore::new(0)),
            released: Arc::new(Semaphore::new(0)),
            triggered: AtomicBool::new(false),
        })
    }

    pub async fn wait(&self) -> TestResult {
        let permit = tokio::time::timeout(
            Duration::from_secs(10),
            Arc::clone(&self.reached).acquire_owned(),
        )
        .await
        .map_err(|_| std::io::Error::other("transfer checkpoint was not reached"))??;
        permit.forget();
        Ok(())
    }

    pub fn release(&self) {
        self.released.add_permits(1);
    }
}

impl TransferCheckpointObserver for CheckpointGate {
    fn checkpoint(&self, point: TransferCheckpointPoint) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if point != self.point || self.triggered.swap(true, Ordering::SeqCst) {
                return;
            }
            self.reached.add_permits(1);
            if let Ok(permit) = Arc::clone(&self.released).acquire_owned().await {
                permit.forget();
            }
        })
    }
}
