use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, TaskId, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, JitterSample, LifecycleFailure, LifecycleService, PublicationIntent,
};
use crate::paths::{PathError, PublicationArtifact};
use crate::persistence::{AttemptRecord, TaskRecord};

use super::download_artifact::{recover_verified, VerifiedArtifactInspection};
use super::publication_artifact::{inspect_source, matches_file};
use super::publication_failure::publication_evidence;
use super::{PublicationOutcome, TransferError, TransferExecutor};

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
        let Some(expected) = publication_evidence(task) else {
            return self.fail_verification(task, attempt, now).await;
        };
        let Ok(Some(workspace)) = self.resources.paths.temp_workspace(task.id, false) else {
            return self.fail_verification(task, attempt, now).await;
        };
        let Ok(Some(artifact)) = recover_verified(&workspace, task.output_extension.as_str()).await
        else {
            return self.fail_verification(task, attempt, now).await;
        };
        if artifact.size != expected.size || artifact.sha256 != expected.sha256 {
            return self.fail_verification(task, attempt, now).await;
        }
        match self
            .resources
            .paths
            .open_output(task.request.output_path.as_str())
        {
            Ok(output) => {
                if let Err(error) = output.revalidate_missing() {
                    return self.fail_publication_path(task, attempt, error, now).await;
                }
            }
            Err(PathError::OutputExists { .. }) => {
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
            Err(error) => return self.fail_publication_path(task, attempt, error, now).await,
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
        let Some(expected) = publication_evidence(task) else {
            return self.fail_ambiguous(task, attempt, now).await;
        };
        let output = match self
            .resources
            .paths
            .reopen_output(task.request.output_path.as_str())
        {
            Ok(output) => output,
            Err(
                PathError::InvalidPath { .. }
                | PathError::OutsideRoots { .. }
                | PathError::SymlinkComponent { .. }
                | PathError::RootChanged { .. }
                | PathError::OutputParentChanged { .. },
            ) => {
                return self.fail_ambiguous(task, attempt, now).await;
            }
            Err(error) => return self.fail_publication_path(task, attempt, error, now).await,
        };
        if let Some(legacy_staging_name) = task.publication.destination_staging_name.as_deref() {
            match output.open_legacy_staging(legacy_staging_name) {
                Ok(PublicationArtifact::Missing) => {}
                Ok(PublicationArtifact::Regular(_) | PublicationArtifact::NonRegular) | Err(_) => {
                    return self.fail_ambiguous(task, attempt, now).await;
                }
            }
        }
        let Ok(workspace) = self.resources.paths.temp_workspace(task.id, false) else {
            return self.fail_publication(task, attempt, now).await;
        };
        let Ok(artifact) =
            inspect_source(workspace.as_ref(), task.output_extension.as_str(), expected).await
        else {
            return self.fail_publication(task, attempt, now).await;
        };
        match output.open_final() {
            Ok(PublicationArtifact::Regular(final_file)) => {
                if let VerifiedArtifactInspection::Valid(ref artifact) = artifact {
                    return self
                        .move_publication(&output, &artifact.source, task, attempt, expected, now)
                        .await;
                }
                if !matches!(artifact, VerifiedArtifactInspection::Missing) {
                    return self.fail_ambiguous(task, attempt, now).await;
                }
                return match matches_file(final_file, expected.size, expected.sha256).await {
                    Ok(true) => {
                        self.checkpoint(super::TransferCheckpointPoint::PublicationFinalized)
                            .await;
                        output.sync_parent()?;
                        if let Some(workspace) = workspace.as_ref() {
                            workspace
                                .artifact(format!(
                                    "output.{}.verified",
                                    task.output_extension.as_str()
                                ))?
                                .sync_parent()
                                .await?;
                        }
                        Ok(true)
                    }
                    Ok(false) => self.fail_ambiguous(task, attempt, now).await,
                    Err(_) => self.fail_publication(task, attempt, now).await,
                };
            }
            Ok(PublicationArtifact::Missing) => {}
            Ok(PublicationArtifact::NonRegular) => {
                return self.fail_ambiguous(task, attempt, now).await;
            }
            Err(PathError::Io { .. }) => return self.fail_publication(task, attempt, now).await,
            Err(_) => return self.fail_ambiguous(task, attempt, now).await,
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
                return self.fail_ambiguous(task, attempt, now).await;
            }
        };
        if let Err(error) = output.revalidate_missing() {
            return self.fail_publication_path(task, attempt, error, now).await;
        }
        self.finalize(&output, &artifact.source, task, attempt, expected, now)
            .await
    }
}
