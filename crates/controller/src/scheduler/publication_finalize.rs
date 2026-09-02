use std::io::ErrorKind;

use chrono::{DateTime, Utc};

use crate::paths::{PathError, RootedOutput};
use crate::persistence::{AttemptRecord, TaskRecord};

use super::publication_artifact::{matches_file, rename_exclusive, sync_directory};
use super::publication_failure::ExpectedPublication;
use super::{TransferError, TransferExecutor};

impl TransferExecutor {
    pub(super) async fn finalize(
        &self,
        output: &RootedOutput,
        staging_name: &str,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        expected: ExpectedPublication,
        now: DateTime<Utc>,
    ) -> Result<bool, TransferError> {
        let Ok(staging) = output.staging_path(staging_name) else {
            return self.fail_ambiguous(task, attempt, now).await;
        };
        match rename_exclusive(staging, output.display_path().to_path_buf()).await {
            Ok(()) => {
                let parent = output
                    .display_path()
                    .parent()
                    .ok_or_else(|| std::io::Error::other("publication output has no parent"))?;
                if sync_directory(parent).await.is_err() {
                    return self.fail_publication(task, attempt, now).await;
                }
                Ok(true)
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                let final_file = match output.open_final() {
                    Ok(Some(final_file)) => final_file,
                    Err(PathError::Io { .. }) => {
                        return self.fail_publication(task, attempt, now).await;
                    }
                    Ok(None) | Err(_) => return self.fail_ambiguous(task, attempt, now).await,
                };
                match matches_file(final_file, expected.size, expected.sha256).await {
                    Ok(true) => Ok(true),
                    Ok(false) => self.fail_ambiguous(task, attempt, now).await,
                    Err(_) => self.fail_publication(task, attempt, now).await,
                }
            }
            Err(_) => self.fail_publication(task, attempt, now).await,
        }
    }
}
