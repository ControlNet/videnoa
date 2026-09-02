use std::io::ErrorKind;

use chrono::{DateTime, Utc};

use crate::paths::{PathError, PublicationArtifact, RootedOutput};
use crate::persistence::{AttemptRecord, TaskRecord};

use super::publication_artifact::{matches_file, sync_directory};
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
        let staging = match output.open_staging(staging_name) {
            Ok(PublicationArtifact::Regular(staging)) => staging,
            Ok(PublicationArtifact::Missing | PublicationArtifact::NonRegular) | Err(_) => {
                return self.fail_ambiguous(task, attempt, now).await;
            }
        };
        match matches_file(staging, expected.size, expected.sha256).await {
            Ok(true) => {}
            Ok(false) => return self.fail_ambiguous(task, attempt, now).await,
            Err(_) => return self.fail_publication(task, attempt, now).await,
        }
        match output.finalize_staging(staging_name) {
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
            Err(PathError::Io { source, .. }) if source.kind() == ErrorKind::AlreadyExists => {
                let final_file = match output.open_final() {
                    Ok(PublicationArtifact::Regular(final_file)) => final_file,
                    Err(PathError::Io { .. }) => {
                        return self.fail_publication(task, attempt, now).await;
                    }
                    Ok(PublicationArtifact::Missing | PublicationArtifact::NonRegular) | Err(_) => {
                        return self.fail_ambiguous(task, attempt, now).await;
                    }
                };
                match matches_file(final_file, expected.size, expected.sha256).await {
                    Ok(true) => match output.open_staging(staging_name) {
                        Ok(PublicationArtifact::Missing) => Ok(true),
                        Ok(PublicationArtifact::Regular(_) | PublicationArtifact::NonRegular)
                        | Err(_) => self.fail_ambiguous(task, attempt, now).await,
                    },
                    Ok(false) => self.fail_ambiguous(task, attempt, now).await,
                    Err(_) => self.fail_publication(task, attempt, now).await,
                }
            }
            Err(PathError::Io { .. }) => self.fail_publication(task, attempt, now).await,
            Err(_) => self.fail_ambiguous(task, attempt, now).await,
        }
    }
}
