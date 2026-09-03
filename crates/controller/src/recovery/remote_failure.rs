use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, TaskStatus};
use crate::lifecycle::{LifecycleFailure, LifecycleService, RemoteAmbiguityStage};
use crate::persistence::{AttemptRecord, TaskRecord};
use crate::remote::VidenoaClientError;

use super::{Reconciler, RecoveryCommandKind, RecoveryError, RecoveryReport, StagePermit};

#[derive(Clone, Copy)]
pub(super) enum RemoteFailureOperation {
    Submission,
    Processing,
}

struct RemoteFailureContext<'a> {
    task: &'a TaskRecord,
    attempt: &'a AttemptRecord,
    now: DateTime<Utc>,
    stage: &'a StagePermit,
    report: &'a mut RecoveryReport,
}

impl Reconciler {
    pub(super) async fn resolve_submission_failure(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        error: VidenoaClientError,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        self.resolve_remote_failure(
            RemoteFailureOperation::Submission,
            error,
            RemoteFailureContext {
                task,
                attempt,
                now,
                stage,
                report,
            },
        )
        .await
    }

    pub(super) async fn resolve_processing_failure(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        error: VidenoaClientError,
        now: DateTime<Utc>,
        stage: &StagePermit,
        report: &mut RecoveryReport,
    ) -> Result<(), RecoveryError> {
        self.resolve_remote_failure(
            RemoteFailureOperation::Processing,
            error,
            RemoteFailureContext {
                task,
                attempt,
                now,
                stage,
                report,
            },
        )
        .await
    }

    async fn resolve_remote_failure(
        &self,
        operation: RemoteFailureOperation,
        error: VidenoaClientError,
        context: RemoteFailureContext<'_>,
    ) -> Result<(), RecoveryError> {
        let message = error.to_string();
        let task_failure = match error {
            VidenoaClientError::ServerStatus { .. }
            | VidenoaClientError::Network
            | VidenoaClientError::Timeout
            | VidenoaClientError::Stall
            | VidenoaClientError::LocalIo
            | VidenoaClientError::InvalidFilePath
            | VidenoaClientError::EndpointUrl => return Err(error.into()),
            VidenoaClientError::NotFound
            | VidenoaClientError::RateLimited
            | VidenoaClientError::ClientStatus { .. } => match operation {
                RemoteFailureOperation::Submission => LifecycleFailure::terminal(
                    TaskStatus::Submitting,
                    FailureStage::Submission,
                    FailureCode::RemoteSubmissionFailed,
                    message,
                ),
                RemoteFailureOperation::Processing => LifecycleFailure::remote_state_ambiguous(
                    RemoteAmbiguityStage::Processing,
                    message,
                ),
            },
            VidenoaClientError::Conflict
            | VidenoaClientError::UnexpectedStatus { .. }
            | VidenoaClientError::MalformedPayload
            | VidenoaClientError::OversizedPayload { .. } => {
                let stage = match operation {
                    RemoteFailureOperation::Submission => RemoteAmbiguityStage::Submission,
                    RemoteFailureOperation::Processing => RemoteAmbiguityStage::Processing,
                };
                LifecycleFailure::remote_state_ambiguous(stage, message)
            }
        };
        let _write = context.stage.begin_write();
        LifecycleService::new(self.store.clone())
            .fail(
                context.task,
                Some(context.attempt),
                task_failure,
                context.now,
            )
            .await?;
        context
            .report
            .push(context.task.id, RecoveryCommandKind::Terminal);
        Ok(())
    }
}
