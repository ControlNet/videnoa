use std::io::ErrorKind;

use chrono::{DateTime, Utc};

use crate::paths::{PathError, PublicationArtifact, RootedOutput, TempArtifact};
use crate::persistence::{AttemptRecord, TaskRecord};

use super::publication_artifact::matches_file;
use super::publication_failure::ExpectedPublication;
use super::TransferCheckpointPoint;
use super::{TransferError, TransferExecutor};

impl TransferExecutor {
    pub(super) async fn finalize(
        &self,
        output: &RootedOutput,
        source: &TempArtifact,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        expected: ExpectedPublication,
        now: DateTime<Utc>,
    ) -> Result<bool, TransferError> {
        self.checkpoint(TransferCheckpointPoint::BeforeDestinationStaging)
            .await;
        let Ok(Some((source_file, _))) = source.open_read() else {
            return self.fail_ambiguous(task, attempt, now).await;
        };
        match matches_file(source_file, expected.size, expected.sha256).await {
            Ok(true) => {}
            Ok(false) => return self.fail_ambiguous(task, attempt, now).await,
            Err(_) => return self.fail_publication(task, attempt, now).await,
        }
        let Ok(finalizer) = output.prepare_publication(source) else {
            return self.fail_ambiguous(task, attempt, now).await;
        };
        match finalizer.rename_noreplace() {
            Ok(()) => {
                self.checkpoint(TransferCheckpointPoint::PublicationFinalized)
                    .await;
                require_parent_sync(finalizer.sync_parents())?;
                let final_file = match output.open_final() {
                    Ok(PublicationArtifact::Regular(final_file)) => final_file,
                    Ok(PublicationArtifact::Missing | PublicationArtifact::NonRegular) | Err(_) => {
                        return self.fail_ambiguous(task, attempt, now).await;
                    }
                };
                match matches_file(final_file, expected.size, expected.sha256).await {
                    Ok(true) => Ok(true),
                    Ok(false) => self.fail_ambiguous(task, attempt, now).await,
                    Err(_) => self.fail_publication(task, attempt, now).await,
                }
            }
            Err(PathError::Io { source, .. }) if source.kind() == ErrorKind::AlreadyExists => {
                self.fail_ambiguous(task, attempt, now).await
            }
            Err(PathError::CrossFilesystemPublication { .. }) => {
                self.move_publication(output, source, task, attempt, expected, now)
                    .await
            }
            Err(PathError::Io { .. }) => self.fail_publication(task, attempt, now).await,
            Err(_) => self.fail_ambiguous(task, attempt, now).await,
        }
    }
}

fn require_parent_sync(result: Result<(), PathError>) -> Result<(), TransferError> {
    result.map_err(TransferError::from)
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::PathBuf;

    use crate::paths::PathError;

    use super::{require_parent_sync, TransferError};

    #[test]
    fn parent_sync_failure_is_propagated_for_publishing_recovery() {
        let result = require_parent_sync(Err(PathError::Io {
            path: PathBuf::from("output-parent"),
            source: io::Error::other("injected parent sync failure"),
        }));

        assert!(matches!(
            result,
            Err(TransferError::Path(PathError::Io { .. }))
        ));
    }
}
