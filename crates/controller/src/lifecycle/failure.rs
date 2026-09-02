use crate::domain::{FailureCode, FailureInfo, FailureStage, TaskStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownstreamFailure {
    Upload,
    Download,
    Verification,
    Publication,
    LocalCleanup,
    RemoteCleanup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteAmbiguityStage {
    Submission,
    Processing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteTerminalStatus {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LifecycleFailure {
    expected_status: TaskStatus,
    info: FailureInfo,
}

impl LifecycleFailure {
    #[must_use]
    pub fn processing(message: impl Into<String>) -> Self {
        Self::new(
            TaskStatus::Processing,
            FailureStage::Processing,
            FailureCode::ProcessingFailed,
            message,
            true,
        )
    }

    #[must_use]
    pub fn restart_cancelled(message: impl Into<String>) -> Self {
        Self::processing(message)
    }

    #[must_use]
    pub fn remote_state_ambiguous(stage: RemoteAmbiguityStage, message: impl Into<String>) -> Self {
        let (expected_status, failure_stage) = match stage {
            RemoteAmbiguityStage::Submission => (TaskStatus::Submitting, FailureStage::Submission),
            RemoteAmbiguityStage::Processing => (TaskStatus::Processing, FailureStage::Processing),
        };
        Self::new(
            expected_status,
            failure_stage,
            FailureCode::RemoteStateAmbiguous,
            message,
            false,
        )
    }

    #[must_use]
    pub fn publication_ambiguous(message: impl Into<String>) -> Self {
        Self::new(
            TaskStatus::Publishing,
            FailureStage::Publication,
            FailureCode::PublicationAmbiguous,
            message,
            false,
        )
    }

    #[must_use]
    pub fn downstream(stage: DownstreamFailure, message: impl Into<String>) -> Self {
        let (expected_status, failure_stage, failure_code) = match stage {
            DownstreamFailure::Upload => (
                TaskStatus::Uploading,
                FailureStage::Upload,
                FailureCode::TransferFailed,
            ),
            DownstreamFailure::Download => (
                TaskStatus::Downloading,
                FailureStage::Download,
                FailureCode::TransferFailed,
            ),
            DownstreamFailure::Verification => (
                TaskStatus::Verifying,
                FailureStage::Verification,
                FailureCode::VerificationFailed,
            ),
            DownstreamFailure::Publication => (
                TaskStatus::Publishing,
                FailureStage::Publication,
                FailureCode::PublicationFailed,
            ),
            DownstreamFailure::LocalCleanup => (
                TaskStatus::RemoteCleanup,
                FailureStage::LocalCleanup,
                FailureCode::CleanupFailed,
            ),
            DownstreamFailure::RemoteCleanup => (
                TaskStatus::RemoteCleanup,
                FailureStage::RemoteCleanup,
                FailureCode::CleanupFailed,
            ),
        };
        Self::new(expected_status, failure_stage, failure_code, message, true)
    }

    #[must_use]
    pub fn terminal(
        expected_status: TaskStatus,
        stage: FailureStage,
        code: FailureCode,
        message: impl Into<String>,
    ) -> Self {
        Self::new(expected_status, stage, code, message, false)
    }

    #[must_use]
    pub fn info(&self) -> FailureInfo {
        self.info.clone()
    }

    pub(crate) const fn expected_status(&self) -> TaskStatus {
        self.expected_status
    }

    fn new(
        expected_status: TaskStatus,
        failure_stage: FailureStage,
        failure_code: FailureCode,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            expected_status,
            info: FailureInfo {
                failure_stage,
                failure_code,
                message: message.into(),
                retryable,
            },
        }
    }
}
