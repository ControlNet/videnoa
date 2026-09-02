use crate::domain::{FailureCode, FailureInfo, FailureStage, TaskStatus};

use super::Lifecycle;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResumeStage {
    Uploading,
    Downloading,
    Verifying,
    Publishing,
    RemoteCleanup,
}

impl ResumeStage {
    pub(crate) const fn status(self) -> TaskStatus {
        match self {
            Self::Uploading => TaskStatus::Uploading,
            Self::Downloading => TaskStatus::Downloading,
            Self::Verifying => TaskStatus::Verifying,
            Self::Publishing => TaskStatus::Publishing,
            Self::RemoteCleanup => TaskStatus::RemoteCleanup,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryMode {
    NewProcessingAttempt,
    Resume(ResumeStage),
    Blocked,
}

impl Lifecycle {
    #[must_use]
    pub const fn retry_mode(failure: &FailureInfo) -> RetryMode {
        if !failure.retryable {
            return RetryMode::Blocked;
        }
        match failure.failure_code {
            FailureCode::RemoteStateAmbiguous | FailureCode::PublicationAmbiguous => {
                RetryMode::Blocked
            }
            FailureCode::ProcessingFailed => processing(failure.failure_stage),
            FailureCode::TransferFailed => transfer(failure.failure_stage),
            FailureCode::VerificationFailed => verification(failure.failure_stage),
            FailureCode::PublicationFailed => publication(failure.failure_stage),
            FailureCode::CleanupFailed => cleanup(failure.failure_stage),
            FailureCode::InputUnavailable
            | FailureCode::InputChanged
            | FailureCode::OutputExists
            | FailureCode::WorkerUnavailable
            | FailureCode::WorkflowIncompatible
            | FailureCode::RemoteSubmissionFailed
            | FailureCode::Cancelled => RetryMode::Blocked,
        }
    }
}

const fn processing(stage: FailureStage) -> RetryMode {
    match stage {
        FailureStage::Processing => RetryMode::NewProcessingAttempt,
        FailureStage::Reservation
        | FailureStage::Upload
        | FailureStage::Submission
        | FailureStage::Download
        | FailureStage::Verification
        | FailureStage::Publication
        | FailureStage::LocalCleanup
        | FailureStage::RemoteCleanup => RetryMode::Blocked,
    }
}

const fn transfer(stage: FailureStage) -> RetryMode {
    match stage {
        FailureStage::Upload => RetryMode::Resume(ResumeStage::Uploading),
        FailureStage::Download => RetryMode::Resume(ResumeStage::Downloading),
        FailureStage::Reservation
        | FailureStage::Submission
        | FailureStage::Processing
        | FailureStage::Verification
        | FailureStage::Publication
        | FailureStage::LocalCleanup
        | FailureStage::RemoteCleanup => RetryMode::Blocked,
    }
}

const fn verification(stage: FailureStage) -> RetryMode {
    match stage {
        FailureStage::Verification => RetryMode::Resume(ResumeStage::Verifying),
        FailureStage::Reservation
        | FailureStage::Upload
        | FailureStage::Submission
        | FailureStage::Processing
        | FailureStage::Download
        | FailureStage::Publication
        | FailureStage::LocalCleanup
        | FailureStage::RemoteCleanup => RetryMode::Blocked,
    }
}

const fn publication(stage: FailureStage) -> RetryMode {
    match stage {
        FailureStage::Publication => RetryMode::Resume(ResumeStage::Publishing),
        FailureStage::Reservation
        | FailureStage::Upload
        | FailureStage::Submission
        | FailureStage::Processing
        | FailureStage::Download
        | FailureStage::Verification
        | FailureStage::LocalCleanup
        | FailureStage::RemoteCleanup => RetryMode::Blocked,
    }
}

const fn cleanup(stage: FailureStage) -> RetryMode {
    match stage {
        FailureStage::LocalCleanup | FailureStage::RemoteCleanup => {
            RetryMode::Resume(ResumeStage::RemoteCleanup)
        }
        FailureStage::Reservation
        | FailureStage::Upload
        | FailureStage::Submission
        | FailureStage::Processing
        | FailureStage::Download
        | FailureStage::Verification
        | FailureStage::Publication => RetryMode::Blocked,
    }
}
