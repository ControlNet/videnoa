use chrono::{DateTime, Utc};

use crate::lifecycle::JitterSample;
use crate::persistence::{AttemptRecord, TaskRecord};
use crate::remote::{FileApiPath, VidenoaClient};

use super::{TransferError, TransferExecutor, UploadOutcome};

pub(super) struct UploadContext<'a> {
    pub task: &'a TaskRecord,
    pub attempt: &'a AttemptRecord,
    pub client: &'a VidenoaClient,
    pub api_path: &'a FileApiPath,
    pub now: DateTime<Utc>,
    pub jitter: JitterSample,
}

impl TransferExecutor {
    pub(super) async fn upload_fresh(
        &self,
        context: UploadContext<'_>,
    ) -> Result<UploadOutcome, TransferError> {
        let file = match super::upload_input::open_verified(&self.resources.paths, context.task) {
            Ok(file) => tokio::fs::File::from_std(file.into_std()),
            Err(code) => {
                return self
                    .upload_input_failure(context.task, context.attempt, code, context.now)
                    .await;
            }
        };
        let uploaded = context
            .client
            .upload(context.api_path, context.task.input_size, file)
            .await;
        if let Err(error) = &uploaded {
            tracing::warn!(task_id = %context.task.id, attempt_id = %context.attempt.attempt.id, error = %error, "Upload request failed; checking remote file before retry");
        }
        let stat = context.client.stat(context.api_path).await;
        match stat {
            Ok(stat) if stat.is_file && stat.size == context.task.input_size => {
                let remote_input_path = match uploaded {
                    Ok(receipt) if receipt.size == context.task.input_size => receipt.path,
                    Ok(_) | Err(_) => stat.path,
                };
                self.finish_upload(
                    context.task,
                    context.attempt,
                    remote_input_path,
                    context.now,
                )
                .await
            }
            Ok(stat) => {
                tracing::warn!(task_id = %context.task.id, expected_bytes = context.task.input_size, actual_bytes = stat.size, is_file = stat.is_file, "Uploaded file does not match input");
                self.cleanup_and_retry(
                    context.client,
                    context.api_path,
                    context.task,
                    context.attempt,
                    context.now,
                    context.jitter,
                )
                .await
            }
            Err(error) => {
                tracing::warn!(task_id = %context.task.id, error = %error, "Upload verification request failed");
                self.upload_retry(context.task, context.attempt, context.now, context.jitter)
                    .await
            }
        }
    }
}
