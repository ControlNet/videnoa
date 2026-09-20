use chrono::{DateTime, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::paths::{PublicationArtifact, RootedOutput, TempArtifact};
use crate::persistence::{AttemptRecord, TaskRecord};

use super::diagnostics::{Diagnose, OperationError};
use super::publication_artifact::matches_file;
use super::publication_failure::ExpectedPublication;
use super::{TransferCheckpointPoint, TransferError, TransferExecutor};

const COPY_BYTES: usize = 64 * 1024;
const MARKER_NAME: &str = "publication-copy.evidence";
const MARKER_BYTES: usize = 64;
const MAGIC: &[u8; 8] = b"VDNMOVE1";

impl TransferExecutor {
    pub(super) async fn move_publication(
        &self,
        output: &RootedOutput,
        source: &TempArtifact,
        task: &TaskRecord,
        attempt: &AttemptRecord,
        expected: ExpectedPublication,
        now: DateTime<Utc>,
    ) -> Result<bool, TransferError> {
        match self.copy_publication(output, source, expected).await {
            Ok(()) => Ok(true),
            Err(error)
                if matches!(
                    error.source,
                    TransferError::Conflict
                        | TransferError::Path(crate::paths::PathError::OutputExists { .. })
                ) =>
            {
                self.fail_ambiguous(task, attempt, now, error).await
            }
            Err(error) => self.fail_publication(task, attempt, now, error).await,
        }
    }

    async fn copy_publication(
        &self,
        output: &RootedOutput,
        source: &TempArtifact,
        expected: ExpectedPublication,
    ) -> Result<(), OperationError> {
        let marker = source.sibling(MARKER_NAME).at("copy.marker_path")?;
        let ownership = match output.open_final().at("copy.open_destination")? {
            PublicationArtifact::Missing => None,
            PublicationArtifact::Regular(_) => Some(
                read_marker(&marker, expected)
                    .await?
                    .ok_or_else(|| OperationError::conflict("copy.marker_missing"))?,
            ),
            PublicationArtifact::NonRegular => {
                return Err(OperationError::conflict("copy.destination_not_regular"))
            }
        };
        let (source_file, _) = source
            .open_read()
            .at("copy.open_source")?
            .ok_or_else(|| OperationError::conflict("copy.source_missing"))?;
        if !matches_file(source_file, expected.size, expected.sha256)
            .await
            .at("copy.hash_source")?
        {
            return Err(OperationError::conflict("copy.source_content_mismatch"));
        }
        // Opening a second source handle avoids sharing the hash reader's file offset.
        let (source_file, _) = source
            .open_read()
            .at("copy.open_source")?
            .ok_or_else(|| OperationError::conflict("copy.source_missing"))?;
        let mut destination = output
            .open_copy(ownership)
            .at("copy.open_owned_destination")?;
        self.checkpoint(TransferCheckpointPoint::PublicationCopyCreated)
            .await;
        let file = destination.file();
        let identity = RootedOutput::copy_identity(file).at("copy.destination_identity")?;
        let length = file.metadata().at("copy.destination_metadata")?.len();
        if length > expected.size {
            return Err(OperationError::conflict("copy.destination_too_long"));
        }
        destination
            .validate_visible(output)
            .at("copy.validate_destination")?;
        if ownership.is_none() {
            destination
                .file()
                .sync_all()
                .at("copy.sync_empty_destination")?;
            destination
                .sync_parent()
                .at("copy.sync_destination_parent")?;
            write_marker(&marker, identity, expected).await?;
        }
        self.checkpoint(TransferCheckpointPoint::PublicationCopyStarted)
            .await;
        let copy_file = destination
            .file()
            .try_clone()
            .at("copy.clone_destination")?;
        self.copy_suffix(source_file, copy_file, length, expected.size)
            .await?;
        destination.keep();
        let PublicationArtifact::Regular(final_file) =
            output.open_copy_final().at("copy.open_destination")?
        else {
            return Err(OperationError::conflict("copy.final_not_regular"));
        };
        if RootedOutput::copy_identity(&final_file).at("copy.final_identity")? != identity
            || !matches_file(final_file, expected.size, expected.sha256)
                .await
                .at("copy.hash_destination")?
        {
            return Err(OperationError::conflict(
                "copy.final_identity_or_content_mismatch",
            ));
        }
        destination
            .sync_parent()
            .at("copy.sync_destination_parent")?;
        self.checkpoint(TransferCheckpointPoint::PublicationCopyVerified)
            .await;
        // Keep the verified source until the destination is durable and fully validated.
        let PublicationArtifact::Regular(final_file) =
            output.open_copy_final().at("copy.open_destination")?
        else {
            return Err(OperationError::conflict(
                "copy.final_not_regular_before_source_removal",
            ));
        };
        if RootedOutput::copy_identity(&final_file).at("copy.final_identity")? != identity {
            return Err(OperationError::conflict(
                "copy.final_identity_changed_before_source_removal",
            ));
        }
        source.remove().at("copy.remove_verified_source")?;
        source.sync_parent().await.at("copy.sync_source_parent")?;
        self.checkpoint(TransferCheckpointPoint::PublicationFinalized)
            .await;
        Ok(())
    }

    async fn copy_suffix(
        &self,
        source: cap_std::fs::File,
        destination: cap_std::fs::File,
        prefix: u64,
        expected_size: u64,
    ) -> Result<(), OperationError> {
        let mut source = tokio::fs::File::from_std(source.into_std());
        let mut destination = tokio::fs::File::from_std(destination.into_std());
        let mut source_bytes = vec![0; COPY_BYTES];
        let mut destination_bytes = vec![0; COPY_BYTES];
        let mut remaining = prefix;
        // Never truncate an interrupted output: validate the owned prefix before appending.
        while remaining > 0 {
            let length = usize::try_from(remaining.min(COPY_BYTES as u64))
                .map_err(std::io::Error::other)
                .at("copy.prefix_length")?;
            source
                .read_exact(&mut source_bytes[..length])
                .await
                .at("copy.read_source_prefix")?;
            destination
                .read_exact(&mut destination_bytes[..length])
                .await
                .at("copy.read_destination_prefix")?;
            if source_bytes[..length] != destination_bytes[..length] {
                return Err(OperationError::conflict("copy.prefix_content_mismatch"));
            }
            remaining -= length as u64;
        }
        let mut written = prefix;
        loop {
            let length = source
                .read(&mut source_bytes)
                .await
                .at("copy.read_source")?;
            if length == 0 {
                break;
            }
            written = written
                .checked_add(length as u64)
                .ok_or_else(|| OperationError::conflict("copy.length_overflow"))?;
            if written > expected_size {
                return Err(OperationError::conflict(
                    "copy.source_exceeds_expected_size",
                ));
            }
            destination
                .write_all(&source_bytes[..length])
                .await
                .at("copy.write_destination")?;
            destination.flush().await.at("copy.flush_destination")?;
            self.checkpoint(TransferCheckpointPoint::PublicationCopyChunkWritten)
                .await;
        }
        if written != expected_size {
            return Err(OperationError::conflict("copy.source_size_mismatch"));
        }
        destination
            .sync_all()
            .await
            .at("copy.sync_completed_destination")?;
        Ok(())
    }
}

async fn write_marker(
    marker: &TempArtifact,
    identity: [u8; 16],
    expected: ExpectedPublication,
) -> Result<(), OperationError> {
    let mut bytes = [0; MARKER_BYTES];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..24].copy_from_slice(&identity);
    bytes[24..32].copy_from_slice(&expected.size.to_be_bytes());
    bytes[32..].copy_from_slice(expected.sha256.as_bytes());
    let pending = marker
        .sibling("publication-copy.pending")
        .at("copy.pending_marker_path")?;
    let mut file = tokio::fs::File::from_std(
        pending
            .create_truncated()
            .at("copy.create_pending_marker")?
            .into_std(),
    );
    file.write_all(&bytes).await.at("copy.write_marker")?;
    file.sync_all().await.at("copy.sync_marker")?;
    drop(file);
    pending.rename_to(marker).await.at("copy.install_marker")?;
    Ok(())
}

async fn read_marker(
    marker: &TempArtifact,
    expected: ExpectedPublication,
) -> Result<Option<[u8; 16]>, OperationError> {
    let Some((file, metadata)) = marker.open_read().at("copy.open_marker")? else {
        return Ok(None);
    };
    if metadata.len() != MARKER_BYTES as u64 {
        return Err(OperationError::conflict("copy.marker_length_mismatch"));
    }
    let mut bytes = [0; MARKER_BYTES];
    tokio::fs::File::from_std(file.into_std())
        .read_exact(&mut bytes)
        .await
        .at("copy.read_marker")?;
    if &bytes[..8] != MAGIC
        || bytes[24..32] != expected.size.to_be_bytes()
        || &bytes[32..] != expected.sha256.as_bytes()
    {
        return Err(OperationError::conflict("copy.marker_evidence_mismatch"));
    }
    let mut identity = [0; 16];
    identity.copy_from_slice(&bytes[8..24]);
    Ok(Some(identity))
}
