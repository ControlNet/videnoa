use std::sync::Arc;

use futures_util::future::{ready, BoxFuture, FutureExt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferCheckpointPoint {
    StagingVerified,
    PublicationFinalized,
    BeforeLocalCleanup,
    BeforeRemoteDelete,
    RemoteDeleteSucceeded,
}

pub trait TransferCheckpointObserver: Send + Sync {
    fn checkpoint(&self, point: TransferCheckpointPoint) -> BoxFuture<'_, ()>;
}

pub(super) struct NoopTransferCheckpointObserver;

impl TransferCheckpointObserver for NoopTransferCheckpointObserver {
    fn checkpoint(&self, _point: TransferCheckpointPoint) -> BoxFuture<'_, ()> {
        ready(()).boxed()
    }
}

pub(super) fn noop_observer() -> Arc<dyn TransferCheckpointObserver> {
    Arc::new(NoopTransferCheckpointObserver)
}
