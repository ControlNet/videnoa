use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::future::BoxFuture;
use tokio::sync::Semaphore;
use videnoa_controller::scheduler::{TransferCheckpointObserver, TransferCheckpointPoint};

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

    pub async fn wait(&self) -> Result<(), tokio::sync::AcquireError> {
        let permit = Arc::clone(&self.reached).acquire_owned().await?;
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
