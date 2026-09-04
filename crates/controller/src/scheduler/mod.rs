#[path = "transfer_checkpoint.rs"]
mod checkpoints;
mod cleanup;
mod cleanup_remote;
#[path = "error.rs"]
mod dispatch_error;
mod download;
mod download_artifact;
mod hashing_writer;
mod model;
#[path = "transfer.rs"]
mod permits;
mod publication;
mod publication_artifact;
mod publication_failure;
mod publication_finalize;
#[path = "service.rs"]
mod queue;
mod recovery_dispatch;
mod runtime_settings;
mod transfer_executor;
mod upload;
mod upload_fresh;

pub(crate) use checkpoints::noop_observer;
pub use checkpoints::{TransferCheckpointObserver, TransferCheckpointPoint};
pub(crate) use cleanup::remove_task_workspace;
pub use dispatch_error::{SchedulerError, SchedulerErrorCode, TransferError, TransferLimitError};
use hashing_writer::HashingWriter;
pub use model::{AssignmentClass, ScheduledAssignment, UploadCandidate, UploadPriority};
pub use permits::{DownloadPermit, TransferCoordinator, UploadPermit};
pub use queue::Scheduler;
pub use runtime_settings::RuntimeSettings;
use transfer_executor::RetryResult;
pub use transfer_executor::{
    DownloadOutcome, PublicationOutcome, TransferConfig, TransferExecutor, TransferResources,
    UploadOutcome, VerifiedArtifact,
};
