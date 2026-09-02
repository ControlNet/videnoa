mod download;
mod error;
mod hashing_writer;
mod model;
mod service;
mod transfer;
mod transfer_executor;
mod upload;

pub use error::{SchedulerError, SchedulerErrorCode, TransferError, TransferLimitError};
use hashing_writer::HashingWriter;
pub use model::{AssignmentClass, ScheduledAssignment, UploadCandidate, UploadPriority};
pub use service::Scheduler;
pub use transfer::{DownloadPermit, TransferCoordinator, UploadPermit};
use transfer_executor::RetryResult;
pub use transfer_executor::{
    DownloadOutcome, TransferConfig, TransferExecutor, TransferResources, UploadOutcome,
    VerifiedArtifact,
};
