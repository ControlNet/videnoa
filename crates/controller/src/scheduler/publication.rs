use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, TaskId, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, JitterSample, LifecycleFailure, LifecycleService, PublicationIntent,
};
use crate::paths::{PathError, PublicationArtifact, RootedOutput, TempWorkspace};
use crate::persistence::{AttemptRecord, TaskRecord};

use super::diagnostics::{Diagnose, OperationError};
use super::download_artifact::{recover_verified, VerifiedArtifactInspection};
use super::publication_artifact::{inspect_source, matches_file};
use super::publication_failure::{publication_evidence, ExpectedPublication};
use super::{PublicationOutcome, TransferError, TransferExecutor};

struct PublicationContext<'a> {
    task: &'a TaskRecord,
    attempt: &'a AttemptRecord,
    now: DateTime<Utc>,
    expected: ExpectedPublication,
}

impl TransferExecutor {
    /// Publishes verified output without replacing an existing destination, then converges cleanup.
    ///
    /// # Errors
    /// Returns [`TransferError`] when durable snapshots or lifecycle writes cannot be completed.
    pub async fn publish(
        &self,
        task_id: TaskId,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<PublicationOutcome, TransferError> {
        // Keep duplicate finalizers from interpreting an in-flight rename as missing input.
        let _permit = self
            .resources
            .coordinator
            .try_publish(task_id)
            .ok_or(TransferError::Busy)?;
        let (mut task, mut attempt) = self.snapshots(task_id).await?;
        Self::require_retry_due(&task, &attempt, now)?;
        match task.status {
            TaskStatus::Verifying => {
                if !self.admit_publication(&task, &attempt, now).await? {
                    return Ok(PublicationOutcome::Failed);
                }
                (task, attempt) = self.snapshots(task_id).await?;
            }
            TaskStatus::Publishing => {}
            TaskStatus::RemoteCleanup => return self.cleanup(task_id, now, jitter).await,
            TaskStatus::Queued
            | TaskStatus::Reserved
            | TaskStatus::Uploading
            | TaskStatus::Staged
            | TaskStatus::Submitting
            | TaskStatus::Processing
            | TaskStatus::RemoteCompleted
            | TaskStatus::Downloading
            | TaskStatus::Completed
            | TaskStatus::Failed
            | TaskStatus::Cancelled => return Err(TransferError::Conflict),
        }
        if !self.reconcile_publication(&task, &attempt, now).await? {
            return Ok(PublicationOutcome::Failed);
        }
        let (task, attempt) = self.snapshots(task_id).await?;
        LifecycleService::new(self.resources.store.clone())
            .advance(&task, &attempt, AdvanceCommand::FinishPublication, now)
            .await?;
        self.cleanup(task_id, now, jitter).await
    }

    async fn admit_publication(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
    ) -> Result<bool, TransferError> {
        let path_failed = |error| self.fail_publication_path(task, attempt, now, error);
        let verification_failed = |error| self.fail_verification(task, attempt, now, error);
        let Some(expected) = publication_evidence(task) else {
            return verification_failed(OperationError::conflict(
                "verify.expected_evidence_missing",
            ))
            .await;
        };
        let workspace = match self.resources.paths.temp_workspace(task.id, false) {
            Ok(Some(workspace)) => workspace,
            Ok(None) => {
                return verification_failed(OperationError::conflict("verify.workspace_missing"))
                    .await
            }
            Err(error) => {
                return verification_failed(OperationError::new("verify.open_workspace", error))
                    .await
            }
        };
        let artifact = match recover_verified(&workspace, task.output_extension.as_str()).await {
            Ok(Some(artifact)) => artifact,
            Ok(None) => {
                return verification_failed(OperationError::conflict(
                    "verify.artifact_missing_or_invalid",
                ))
                .await
            }
            Err(error) => {
                return verification_failed(OperationError::new("verify.recover_artifact", error))
                    .await
            }
        };
        if artifact.size != expected.size || artifact.sha256 != expected.sha256 {
            return verification_failed(OperationError::conflict(
                "verify.artifact_content_mismatch",
            ))
            .await;
        }
        match self
            .resources
            .paths
            .open_output(task.request.output_path.as_str())
        {
            Ok(output) => {
                if let Err(error) = output.revalidate_missing() {
                    return path_failed(OperationError::new(
                        "publication.revalidate_destination",
                        error,
                    ))
                    .await;
                }
            }
            Err(error @ PathError::OutputExists { .. }) => {
                OperationError::new("verify.open_destination", error).log(
                    task.id,
                    Some(attempt.attempt.id),
                    task.status,
                );
                LifecycleService::new(self.resources.store.clone())
                    .fail(
                        task,
                        Some(attempt),
                        LifecycleFailure::terminal(
                            TaskStatus::Verifying,
                            FailureStage::Publication,
                            FailureCode::OutputExists,
                            "publication destination already exists",
                        ),
                        now,
                    )
                    .await?;
                return Ok(false);
            }
            Err(error) => {
                return path_failed(OperationError::new("verify.open_destination", error)).await
            }
        }
        LifecycleService::new(self.resources.store.clone())
            .advance(
                task,
                attempt,
                AdvanceCommand::FinishVerification(PublicationIntent::direct()),
                now,
            )
            .await?;
        Ok(true)
    }

    async fn reconcile_publication(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
    ) -> Result<bool, TransferError> {
        let ambiguous = |error| self.fail_ambiguous(task, attempt, now, error);
        let failed = |error| self.fail_publication(task, attempt, now, error);
        let path_failed = |error| self.fail_publication_path(task, attempt, now, error);
        let Some(expected) = publication_evidence(task) else {
            return ambiguous(OperationError::conflict(
                "publication.expected_evidence_missing",
            ))
            .await;
        };
        let Some(output) = self.reopen_publication_output(task, attempt, now).await? else {
            return Ok(false);
        };
        let workspace = match self.resources.paths.temp_workspace(task.id, false) {
            Ok(workspace) => workspace,
            Err(error) => {
                return failed(OperationError::new("publication.open_workspace", error)).await
            }
        };
        let artifact = match inspect_source(
            workspace.as_ref(),
            task.output_extension.as_str(),
            expected,
        )
        .await
        {
            Ok(artifact) => artifact,
            Err(error) => {
                return failed(OperationError::new(
                    "publication.inspect_verified_source",
                    error,
                ))
                .await
            }
        };
        match output.open_final() {
            Ok(PublicationArtifact::Regular(final_file)) => {
                return self
                    .recover_existing_final(
                        &output,
                        final_file,
                        &artifact,
                        workspace.as_ref(),
                        PublicationContext {
                            task,
                            attempt,
                            now,
                            expected,
                        },
                    )
                    .await;
            }
            Ok(PublicationArtifact::Missing) => {}
            Ok(PublicationArtifact::NonRegular) => {
                return ambiguous(OperationError::conflict("publication.final_not_regular")).await;
            }
            Err(error @ PathError::Io { .. }) => {
                return failed(OperationError::new("publication.open_final", error)).await
            }
            Err(error) => {
                return ambiguous(OperationError::new("publication.open_final", error)).await
            }
        }
        let artifact = match artifact {
            VerifiedArtifactInspection::Valid(artifact)
                if artifact.size == expected.size && artifact.sha256 == expected.sha256 =>
            {
                *artifact
            }
            VerifiedArtifactInspection::Missing
            | VerifiedArtifactInspection::Invalid
            | VerifiedArtifactInspection::Valid(_) => {
                return ambiguous(OperationError::conflict(
                    "publication.verified_source_missing_or_invalid",
                ))
                .await;
            }
        };
        if let Err(error) = output.revalidate_missing() {
            return path_failed(OperationError::new(
                "publication.revalidate_destination",
                error,
            ))
            .await;
        }
        self.finalize(&output, &artifact.source, task, attempt, expected, now)
            .await
    }
    async fn reopen_publication_output(
        &self,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        now: DateTime<Utc>,
    ) -> Result<Option<RootedOutput>, TransferError> {
        let ambiguous = |error| self.fail_ambiguous(task, attempt, now, error);
        let path_failed = |error| self.fail_publication_path(task, attempt, now, error);
        let output = match self
            .resources
            .paths
            .reopen_output(task.request.output_path.as_str())
        {
            Ok(output) => output,
            Err(
                error @ (PathError::InvalidPath { .. }
                | PathError::OutsideRoots { .. }
                | PathError::SymlinkComponent { .. }
                | PathError::RootChanged { .. }
                | PathError::OutputParentChanged { .. }),
            ) => {
                ambiguous(OperationError::new("publication.reopen_destination", error)).await?;
                return Ok(None);
            }
            Err(error) => {
                path_failed(OperationError::new("publication.reopen_destination", error)).await?;
                return Ok(None);
            }
        };
        if let Some(legacy_staging_name) = task.publication.destination_staging_name.as_deref() {
            match output.open_legacy_staging(legacy_staging_name) {
                Ok(PublicationArtifact::Missing) => {}
                Ok(PublicationArtifact::Regular(_) | PublicationArtifact::NonRegular) => {
                    ambiguous(OperationError::conflict(
                        "publication.legacy_staging_present",
                    ))
                    .await?;
                    return Ok(None);
                }
                Err(error) => {
                    ambiguous(OperationError::new(
                        "publication.open_legacy_staging",
                        error,
                    ))
                    .await?;
                    return Ok(None);
                }
            }
        }
        Ok(Some(output))
    }

    async fn recover_existing_final(
        &self,
        output: &RootedOutput,
        final_file: cap_std::fs::File,
        artifact: &VerifiedArtifactInspection,
        workspace: Option<&TempWorkspace>,
        context: PublicationContext<'_>,
    ) -> Result<bool, TransferError> {
        let PublicationContext {
            task,
            attempt,
            now,
            expected,
        } = context;
        let ambiguous = |error| self.fail_ambiguous(task, attempt, now, error);
        let failed = |error| self.fail_publication(task, attempt, now, error);
        if let VerifiedArtifactInspection::Valid(artifact) = artifact {
            return self
                .move_publication(output, &artifact.source, task, attempt, expected, now)
                .await;
        }
        if !matches!(artifact, VerifiedArtifactInspection::Missing) {
            return ambiguous(OperationError::conflict(
                "publication.source_invalid_with_existing_final",
            ))
            .await;
        }
        match matches_file(final_file, expected.size, expected.sha256).await {
            Ok(true) => {
                self.checkpoint(super::TransferCheckpointPoint::PublicationFinalized)
                    .await;
                output
                    .sync_parent()
                    .at("publication.sync_recovered_destination_parent")
                    .map_err(|error| error.logged(task.id, attempt.attempt.id, task.status))?;
                if let Some(workspace) = workspace {
                    workspace
                        .artifact(format!(
                            "output.{}.verified",
                            task.output_extension.as_str()
                        ))
                        .at("publication.recovered_source_path")
                        .map_err(|error| error.logged(task.id, attempt.attempt.id, task.status))?
                        .sync_parent()
                        .await
                        .at("publication.sync_recovered_source_parent")
                        .map_err(|error| error.logged(task.id, attempt.attempt.id, task.status))?;
                }
                Ok(true)
            }
            Ok(false) => {
                ambiguous(OperationError::conflict(
                    "publication.final_content_mismatch",
                ))
                .await
            }
            Err(error) => failed(OperationError::new("publication.hash_final", error)).await,
        }
    }
}
