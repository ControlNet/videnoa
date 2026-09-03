use std::sync::Arc;

use futures_util::future::{ready, BoxFuture, FutureExt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferCheckpointPoint {
    UploadCompleted,
    BeforeRemoteSubmit,
    RemoteCompletionPersisted,
    DownloadVerified,
    BeforeDestinationStaging,
    DestinationStaged,
    StagingVerified,
    PublicationFinalized,
    BeforeLocalCleanup,
    LocalCleanupCompleted,
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

pub(crate) fn noop_observer() -> Arc<dyn TransferCheckpointObserver> {
    Arc::new(NoopTransferCheckpointObserver)
}
