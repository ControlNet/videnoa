use chrono::{DateTime, Utc};
use tokio::io::AsyncWriteExt;

use crate::domain::{TaskId, TaskStatus};
use crate::lifecycle::{
    AdvanceCommand, AutomaticRetry, DownloadEvidence, DownstreamFailure, JitterSample,
    LifecycleService,
};
use crate::remote::{FileApiPath, VidenoaClient};

use super::{
    DownloadOutcome, HashingWriter, RetryResult, TransferError, TransferExecutor, VerifiedArtifact,
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
        let client = VidenoaClient::new(
            worker.api_url,
            self.config.remote_timeouts,
            self.config.payload_limits,
        )?;
        let remote_path = FileApiPath::parse(&format!(
            "{}/output.{}",
            task.id,
            task.output_extension.as_str()
        ))?;
        let stat = match client.stat(&remote_path).await {
            Ok(stat) if stat.is_file && stat.size > 0 => stat,
            Ok(_) | Err(_) => return self.download_retry(&task, &attempt, now, jitter).await,
        };
        let directory = self.config.temp_root.join(task.id.to_string());
        tokio::fs::create_dir_all(&directory).await?;
        let part = directory.join(format!("output.{}.part", task.output_extension.as_str()));
        let verified = directory.join(format!(
            "output.{}.verified",
            task.output_extension.as_str()
        ));
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&part)
            .await?;
        let mut writer = HashingWriter::new(file);
        let downloaded = client.download(&remote_path, &mut writer).await;
        if downloaded.is_err() {
            drop(writer);
            remove_part(&part).await?;
            return self.download_retry(&task, &attempt, now, jitter).await;
        }
        writer.flush().await?;
        let (file, size, sha256) = writer.finish();
        if size != stat.size {
            drop(file);
            remove_part(&part).await?;
            return self.download_retry(&task, &attempt, now, jitter).await;
        }
        file.sync_all().await?;
        tokio::fs::rename(&part, &verified).await?;
        let artifact = VerifiedArtifact {
            path: verified,
            size,
            sha256,
        };
        LifecycleService::new(self.resources.store.clone())
            .advance(
                &task,
                &attempt,
                AdvanceCommand::FinishDownload(DownloadEvidence { size, sha256 }),
                now,
            )
            .await?;
        Ok(DownloadOutcome::Verified(artifact))
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

async fn remove_part(path: &std::path::Path) -> Result<(), std::io::Error> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
