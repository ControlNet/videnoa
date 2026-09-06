use crate::domain::{FailureCode, FailureStage, TaskId, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, AutomaticRetry, DownloadEvidence, DownstreamFailure, JitterSample,
    LifecycleFailure, LifecycleService,
};
use crate::remote::{sibling_output_path, FileApiPath, VidenoaClient};
use chrono::{DateTime, Utc};

use super::{
    download_artifact::{download_artifact, recover_verified, DownloadArtifact},
    DownloadOutcome, RetryResult, TransferCheckpointPoint, TransferError, TransferExecutor,
    VerifiedArtifact,
};

impl TransferExecutor {
    /// Streams and verifies one durable download stage.
    ///
    /// # Errors
    /// Returns [`TransferError`] when state, persistence, local artifact I/O, or remote I/O fails.
    pub async fn download(
        &self,
        task_id: TaskId,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<DownloadOutcome, TransferError> {
        let (mut task, mut attempt) = self.snapshots(task_id).await?;
        Self::require_retry_due(&task, &attempt, now)?;
        let Some(remote_output_path) = attempt.attempt.remote_output_path.clone() else {
            return self.download_ambiguity(&task, &attempt, now).await;
        };
        let Some(remote_input_path) = attempt.attempt.remote_input_path.as_ref() else {
            return self.download_ambiguity(&task, &attempt, now).await;
        };
        let output_name = format!("output.{}", task.output_extension.as_str());
        let expected_output_path = sibling_output_path(remote_input_path, &output_name)?;
        if attempt.attempt.remote_job_id.is_none() || remote_output_path != expected_output_path {
            return self.download_ambiguity(&task, &attempt, now).await;
        }
        let _permit = self
            .resources
            .coordinator
            .try_download()
            .ok_or(TransferError::Busy)?;
        match task.status {
            TaskStatus::RemoteCompleted => {
                LifecycleService::new(self.resources.store.clone())
                    .advance(&task, &attempt, AdvanceCommand::StartDownload, now)
                    .await?;
                (task, attempt) = self.snapshots(task_id).await?;
            }
            TaskStatus::Downloading => {}
            TaskStatus::Queued
            | TaskStatus::Reserved
            | TaskStatus::Uploading
            | TaskStatus::Staged
            | TaskStatus::Submitting
            | TaskStatus::Processing
            | TaskStatus::Verifying
            | TaskStatus::Publishing
            | TaskStatus::RemoteCleanup
            | TaskStatus::Completed
            | TaskStatus::Failed
            | TaskStatus::Cancelled => return Err(TransferError::Conflict),
        }
        let Ok((workspace, recovered)) = self.recover_local_artifact(&task).await else {
            return self.download_retry(&task, &attempt, now, jitter).await;
        };
        let artifact = if let Some(artifact) = recovered {
            artifact
        } else {
            let worker_id = attempt
                .attempt
                .worker_id
                .ok_or(TransferError::MissingEvidence)?;
            let worker = self
                .resources
                .store
                .worker(worker_id)
                .await?
                .ok_or(TransferError::MissingEvidence)?;
            let client = VidenoaClient::new_with_password(
                worker.api_url,
                self.config.runtime_settings.remote_timeouts(),
                self.config.payload_limits,
                worker.password.as_ref(),
            )?;
            let remote_path = FileApiPath::parse(&format!("{}/{output_name}", task.id))?;
            let Some(stat) = download_stat(&client, &remote_path, task_id).await else {
                return self.download_retry(&task, &attempt, now, jitter).await;
            };
            let artifact = Box::pin(download_artifact(DownloadArtifact {
                client: &client,
                remote_path: &remote_path,
                workspace,
                extension: task.output_extension.as_str(),
                expected_size: stat.size,
            }))
            .await;
            match artifact {
                Ok(artifact) => artifact,
                Err(error) => {
                    log_download_failure(task_id, &error);
                    return self.download_retry(&task, &attempt, now, jitter).await;
                }
            }
        };
        self.checkpoint(TransferCheckpointPoint::DownloadVerified)
            .await;
        LifecycleService::new(self.resources.store.clone())
            .advance(
                &task,
                &attempt,
                AdvanceCommand::FinishDownload(DownloadEvidence {
                    size: artifact.size,
                    sha256: artifact.sha256,
                }),
                now,
            )
            .await?;
        Ok(DownloadOutcome::Verified(Box::new(artifact)))
    }

    async fn recover_local_artifact(
        &self,
        task: &crate::persistence::TaskRecord,
    ) -> Result<(crate::paths::TempWorkspace, Option<VerifiedArtifact>), TransferError> {
        let workspace = self
            .resources
            .paths
            .temp_workspace(task.id, true)?
            .ok_or(TransferError::MissingEvidence)?;
        let recovered = recover_verified(&workspace, task.output_extension.as_str()).await?;
        Ok((workspace, recovered))
    }

    async fn download_ambiguity(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        now: DateTime<Utc>,
    ) -> Result<DownloadOutcome, TransferError> {
        LifecycleService::new(self.resources.store.clone())
            .fail(
                task,
                Some(attempt),
                LifecycleFailure::terminal(
                    task.status,
                    FailureStage::Download,
                    FailureCode::RemoteStateAmbiguous,
                    "durable download evidence does not identify the remote output",
                ),
                now,
            )
            .await?;
        Ok(DownloadOutcome::Failed)
    }

    async fn download_retry(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        now: DateTime<Utc>,
        jitter: JitterSample,
    ) -> Result<DownloadOutcome, TransferError> {
        Ok(
            match self
                .retry(
                    task,
                    attempt,
                    AutomaticRetry::Download,
                    DownstreamFailure::Download,
                    now,
                    jitter,
                )
                .await?
            {
                RetryResult::Scheduled {
                    retry_count,
                    next_retry_at,
                } => DownloadOutcome::RetryScheduled {
                    retry_count,
                    next_retry_at,
                },
                RetryResult::Failed => DownloadOutcome::Failed,
            },
        )
    }
}

async fn download_stat(
    client: &VidenoaClient,
    path: &FileApiPath,
    task_id: TaskId,
) -> Option<crate::remote::FileStat> {
    match client.stat(path).await {
        Ok(stat) if stat.is_file && stat.size > 0 => Some(stat),
        Ok(_) => {
            tracing::warn!(%task_id, "Remote output is empty or is not a file");
            None
        }
        Err(error) => {
            tracing::warn!(%task_id, error = %error, "Download metadata request failed");
            None
        }
    }
}

fn log_download_failure(task_id: TaskId, error: &TransferError) {
    if let TransferError::Remote(remote) = error {
        tracing::warn!(%task_id, error = %remote, "Download failed");
    } else {
        tracing::warn!(%task_id, error = %error, "Download artifact preparation failed");
    }
}
