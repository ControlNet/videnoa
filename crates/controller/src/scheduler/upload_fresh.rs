use chrono::{DateTime, Utc};

use crate::domain::FailureCode;
use crate::lifecycle::JitterSample;
use crate::persistence::{AttemptRecord, InputIdentity, TaskRecord};
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
        let Ok(rooted) = self
            .resources
            .paths
            .open_input(context.task.request.input_path.as_str())
        else {
            return self
                .upload_input_failure(
                    context.task,
                    context.attempt,
                    FailureCode::InputUnavailable,
                    context.now,
                )
                .await;
        };
        let identity = InputIdentity::new(rooted.snapshot().platform_identity());
        if rooted.snapshot().length != context.task.input_size
            || context.task.input_identity != Some(identity)
            || DateTime::<Utc>::from(rooted.snapshot().modified).timestamp_millis()
                != context.task.input_mtime.timestamp_millis()
        {
            return self
                .upload_input_failure(
                    context.task,
                    context.attempt,
                    FailureCode::InputChanged,
                    context.now,
                )
                .await;
        }
        let file = match rooted.reopen_checked() {
            Ok(file) => tokio::fs::File::from_std(file.into_std()),
            Err(_) => {
                return self
                    .upload_input_failure(
                        context.task,
                        context.attempt,
                        FailureCode::InputChanged,
                        context.now,
                    )
                    .await;
            }
        };
        let uploaded = context
            .client
            .upload(context.api_path, context.task.input_size, file)
            .await;
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
            Ok(_) => {
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
            Err(_) => {
                self.upload_retry(context.task, context.attempt, context.now, context.jitter)
                    .await
            }
        }
    }
}
