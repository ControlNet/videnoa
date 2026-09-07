use std::io::ErrorKind;

use chrono::{DateTime, Utc};

use crate::paths::{PathError, PublicationArtifact, RootedOutput, TempArtifact};
use crate::persistence::{AttemptRecord, TaskRecord};

use super::diagnostics::{Diagnose, OperationError};
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
        let ambiguous = |error| self.fail_ambiguous(task, attempt, now, error);
        let failed = |error| self.fail_publication(task, attempt, now, error);
        self.checkpoint(TransferCheckpointPoint::BeforeDestinationStaging)
            .await;
        let source_file = match source.open_read() {
            Ok(Some((file, _))) => file,
            Ok(None) => return ambiguous(OperationError::conflict("rename.source_missing")).await,
            Err(error) => return ambiguous(OperationError::new("rename.open_source", error)).await,
        };
        match matches_file(source_file, expected.size, expected.sha256).await {
            Ok(true) => {}
            Ok(false) => {
                return ambiguous(OperationError::conflict("rename.source_content_mismatch")).await
            }
            Err(error) => return failed(OperationError::new("rename.hash_source", error)).await,
        }
        let finalizer = match output.prepare_publication(source) {
            Ok(finalizer) => finalizer,
            Err(error) => return ambiguous(OperationError::new("rename.prepare", error)).await,
        };
        match finalizer.rename_noreplace() {
            Ok(()) => {
                self.checkpoint(TransferCheckpointPoint::PublicationFinalized)
                    .await;
                require_parent_sync(finalizer.sync_parents())
                    .at("rename.sync_parents")
                    .map_err(|error| error.logged(task.id, attempt.attempt.id, task.status))?;
                let final_file = match output.open_final() {
                    Ok(PublicationArtifact::Regular(final_file)) => final_file,
                    Ok(PublicationArtifact::Missing | PublicationArtifact::NonRegular) => {
                        return ambiguous(OperationError::conflict(
                            "rename.final_missing_or_not_regular",
                        ))
                        .await;
                    }
                    Err(error) => {
                        return ambiguous(OperationError::new("rename.open_final", error)).await
                    }
                };
                match matches_file(final_file, expected.size, expected.sha256).await {
                    Ok(true) => Ok(true),
                    Ok(false) => {
                        ambiguous(OperationError::conflict("rename.final_content_mismatch")).await
                    }
                    Err(error) => failed(OperationError::new("rename.hash_final", error)).await,
                }
            }
            Err(error @ PathError::Io { .. }) if matches!(&error, PathError::Io { source, .. } if source.kind() == ErrorKind::AlreadyExists) => {
                ambiguous(OperationError::new("rename.noreplace", error)).await
            }
            Err(PathError::CrossFilesystemPublication { .. }) => {
                self.move_publication(output, source, task, attempt, expected, now)
                    .await
            }
            Err(error @ PathError::Io { .. }) => {
                failed(OperationError::new("rename.noreplace", error)).await
            }
            Err(error) => ambiguous(OperationError::new("rename.noreplace", error)).await,
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
