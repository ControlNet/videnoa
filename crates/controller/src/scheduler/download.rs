use crate::domain::{FailureCode, FailureStage, TaskId, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, AutomaticRetry, DownloadEvidence, DownstreamFailure, JitterSample,
    LifecycleFailure, LifecycleService,
};
use crate::remote::{sibling_output_path, FileApiPath, VidenoaClient};
use chrono::{DateTime, Utc};

use super::diagnostics::OperationError;
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
        let started = tokio::time::Instant::now();
        let (mut task, mut attempt) = self.snapshots(task_id).await?;
        Self::require_retry_due(&task, &attempt, now)?;
        let Some(remote_output_path) = attempt.attempt.remote_output_path.clone() else {
            return self
                .download_ambiguity(&task, &attempt, self.observed_now(now, started))
                .await;
        };
        let Some(remote_input_path) = attempt.attempt.remote_input_path.as_ref() else {
            return self
                .download_ambiguity(&task, &attempt, self.observed_now(now, started))
                .await;
        };
        let output_name = format!("output.{}", task.output_extension.as_str());
        let expected_output_path = sibling_output_path(remote_input_path, &output_name)?;
        if attempt.attempt.remote_job_id.is_none() || remote_output_path != expected_output_path {
            return self
                .download_ambiguity(&task, &attempt, self.observed_now(now, started))
                .await;
        }
        let _permit = self
            .resources
            .coordinator
            .try_download()
            .ok_or(TransferError::Busy)?;
        match task.status {
            TaskStatus::RemoteCompleted => {
                LifecycleService::new(self.resources.store.clone())
                    .advance(
                        &task,
                        &attempt,
                        AdvanceCommand::StartDownload,
                        self.observed_now(now, started),
                    )
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
        let Ok((workspace, recovered)) = self.recover_local_artifact(&task, &attempt).await else {
            return self
                .download_retry(&task, &attempt, self.observed_now(now, started), jitter)
                .await;
        };
        let artifact = if let Some(artifact) = recovered {
            artifact
        } else {
            match self
                .fetch_remote_download(
                    &task,
                    &attempt,
                    &output_name,
                    workspace,
                    now,
                    started,
                    jitter,
                )
                .await?
            {
                RemoteFetchOutcome::Verified(artifact) => artifact,
                RemoteFetchOutcome::Retried(outcome) => return Ok(outcome),
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
                self.observed_now(now, started),
            )
            .await?;
        Ok(DownloadOutcome::Verified(Box::new(artifact)))
    }

    #[allow(clippy::too_many_arguments)]
    async fn fetch_remote_download(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
        output_name: &str,
        workspace: crate::paths::TempWorkspace,
        now: DateTime<Utc>,
        started: tokio::time::Instant,
        jitter: JitterSample,
    ) -> Result<RemoteFetchOutcome, TransferError> {
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
        let Some(stat) = download_stat(&client, &remote_path, task.id).await else {
            return self
                .download_retry(task, attempt, self.observed_now(now, started), jitter)
                .await
                .map(RemoteFetchOutcome::Retried);
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
            Ok(artifact) => Ok(RemoteFetchOutcome::Verified(artifact)),
            Err(error) => {
                log_download_failure(task, attempt, error);
                self.download_retry(task, attempt, self.observed_now(now, started), jitter)
                    .await
                    .map(RemoteFetchOutcome::Retried)
            }
        }
    }

    async fn recover_local_artifact(
        &self,
        task: &crate::persistence::TaskRecord,
        attempt: &crate::persistence::AttemptRecord,
    ) -> Result<(crate::paths::TempWorkspace, Option<VerifiedArtifact>), TransferError> {
        let log = |error| {
            OperationError::new("download.recover_local_artifact", error).logged(
                task.id,
                attempt.attempt.id,
                task.status,
            )
        };
        let workspace = self
            .resources
            .paths
            .temp_workspace(task.id, true)
            .map_err(TransferError::from)
            .map_err(log)?
            .ok_or(TransferError::MissingEvidence)
            .map_err(log)?;
        let recovered = recover_verified(&workspace, task.output_extension.as_str())
            .await
            .map_err(log)?;
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

enum RemoteFetchOutcome {
    Verified(VerifiedArtifact),
    Retried(DownloadOutcome),
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

fn log_download_failure(
    task: &crate::persistence::TaskRecord,
    attempt: &crate::persistence::AttemptRecord,
    error: TransferError,
) {
    OperationError::new("download.prepare_artifact", error).log(
        task.id,
        Some(attempt.attempt.id),
        task.status,
    );
}
