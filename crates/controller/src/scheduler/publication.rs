use chrono::{DateTime, Utc};

use crate::domain::{FailureCode, FailureStage, TaskId, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, JitterSample, LifecycleFailure, LifecycleService, PublicationIntent,
};
use crate::paths::{PathError, PublicationArtifact};
use crate::persistence::{AttemptRecord, TaskRecord};

use super::download_artifact::recover_verified;
use super::publication_artifact::{copy_verified, matches_file};
use super::publication_failure::publication_evidence;
use super::{PublicationOutcome, TransferCheckpointPoint, TransferError, TransferExecutor};

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
        let Ok(Some(artifact)) = recover_verified(
            &self.config.temp_root.join(task.id.to_string()),
            task.output_extension.as_str(),
        )
        .await
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
        let intent = PublicationIntent::new(format!(
            ".videnoa-{}-{}.staging",
            task.id,
            uuid::Uuid::new_v4()
        ));
        LifecycleService::new(self.resources.store.clone())
            .advance(
                task,
                attempt,
                AdvanceCommand::FinishVerification(intent),
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
        let Some(staging_name) = task.publication.destination_staging_name.as_deref() else {
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
        match output.open_final() {
            Ok(PublicationArtifact::Regular(final_file)) => {
                return match matches_file(final_file, expected.size, expected.sha256).await {
                    Ok(true) => match output.open_staging(staging_name) {
                        Ok(PublicationArtifact::Missing) => Ok(true),
                        Ok(PublicationArtifact::Regular(_) | PublicationArtifact::NonRegular)
                        | Err(_) => self.fail_ambiguous(task, attempt, now).await,
                    },
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
        match output.open_staging(staging_name) {
            Ok(PublicationArtifact::Regular(staging)) => {
                match matches_file(staging, expected.size, expected.sha256).await {
                    Ok(true) => {}
                    Ok(false) => return self.fail_ambiguous(task, attempt, now).await,
                    Err(_) => return self.fail_publication(task, attempt, now).await,
                }
            }
            Ok(PublicationArtifact::Missing) => {
                let artifact = match recover_verified(
                    &self.config.temp_root.join(task.id.to_string()),
                    task.output_extension.as_str(),
                )
                .await
                {
                    Ok(Some(artifact)) => artifact,
                    Ok(None) => return self.fail_ambiguous(task, attempt, now).await,
                    Err(_) => return self.fail_publication(task, attempt, now).await,
                };
                if artifact.size != expected.size || artifact.sha256 != expected.sha256 {
                    return self.fail_ambiguous(task, attempt, now).await;
                }
                if let Err(error) = output.revalidate_missing() {
                    return self.fail_publication_path(task, attempt, error, now).await;
                }
                self.checkpoint(TransferCheckpointPoint::BeforeDestinationStaging)
                    .await;
                let staging = match output.create_staging(staging_name) {
                    Ok(staging) => staging,
                    Err(error) => {
                        return self.fail_publication_path(task, attempt, error, now).await;
                    }
                };
                if copy_verified(artifact.path, staging, expected.size, expected.sha256)
                    .await
                    .is_err()
                {
                    return self.fail_publication(task, attempt, now).await;
                }
                self.checkpoint(TransferCheckpointPoint::DestinationStaged)
                    .await;
            }
            Ok(PublicationArtifact::NonRegular) => {
                return self.fail_ambiguous(task, attempt, now).await;
            }
            Err(PathError::Io { .. }) => return self.fail_publication(task, attempt, now).await,
            Err(_) => return self.fail_ambiguous(task, attempt, now).await,
        }
        self.finalize(&output, staging_name, task, attempt, expected, now)
            .await
    }
}
