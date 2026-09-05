use chrono::{DateTime, Utc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::paths::{PublicationArtifact, RootedOutput, TempArtifact};
use crate::persistence::{AttemptRecord, TaskRecord};

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
            Err(TransferError::Conflict) => self.fail_ambiguous(task, attempt, now).await,
            Err(TransferError::Path(crate::paths::PathError::OutputExists { .. })) => {
                self.fail_ambiguous(task, attempt, now).await
            }
            Err(_) => self.fail_publication(task, attempt, now).await,
        }
    }

    async fn copy_publication(
        &self,
        output: &RootedOutput,
        source: &TempArtifact,
        expected: ExpectedPublication,
    ) -> Result<(), TransferError> {
        let marker = source.sibling(MARKER_NAME)?;
        let ownership = match output.open_final()? {
            PublicationArtifact::Missing => None,
            PublicationArtifact::Regular(_) => Some(
                read_marker(&marker, expected)
                    .await?
                    .ok_or(TransferError::Conflict)?,
            ),
            PublicationArtifact::NonRegular => return Err(TransferError::Conflict),
        };
        let (source_file, _) = source.open_read()?.ok_or(TransferError::Conflict)?;
        if !matches_file(source_file, expected.size, expected.sha256).await? {
            return Err(TransferError::Conflict);
        }
        // Opening a second source handle avoids sharing the hash reader's file offset.
        let (source_file, _) = source.open_read()?.ok_or(TransferError::Conflict)?;
        let destination = output.open_copy(ownership)?;
        let identity = RootedOutput::copy_identity(&destination)?;
        let length = destination.metadata()?.len();
        if length > expected.size {
            return Err(TransferError::Conflict);
        }
        if ownership.is_none() {
            destination.sync_all()?;
            output.sync_parent()?;
            write_marker(&marker, identity, expected).await?;
        }
        self.checkpoint(TransferCheckpointPoint::PublicationCopyStarted)
            .await;
        self.copy_suffix(source_file, destination, length, expected.size)
            .await?;
        let PublicationArtifact::Regular(final_file) = output.open_final()? else {
            return Err(TransferError::Conflict);
        };
        if RootedOutput::copy_identity(&final_file)? != identity
            || !matches_file(final_file, expected.size, expected.sha256).await?
        {
            return Err(TransferError::Conflict);
        }
        output.sync_parent()?;
        self.checkpoint(TransferCheckpointPoint::PublicationCopyVerified)
            .await;
        // Keep the verified source until the destination is durable and fully validated.
        let PublicationArtifact::Regular(final_file) = output.open_final()? else {
            return Err(TransferError::Conflict);
        };
        if RootedOutput::copy_identity(&final_file)? != identity {
            return Err(TransferError::Conflict);
        }
        source.remove()?;
        source.sync_parent().await?;
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
    ) -> Result<(), TransferError> {
        let mut source = tokio::fs::File::from_std(source.into_std());
        let mut destination = tokio::fs::File::from_std(destination.into_std());
        let mut source_bytes = vec![0; COPY_BYTES];
        let mut destination_bytes = vec![0; COPY_BYTES];
        let mut remaining = prefix;
        // Never truncate an interrupted output: validate the owned prefix before appending.
        while remaining > 0 {
            let length =
                usize::try_from(remaining.min(COPY_BYTES as u64)).map_err(std::io::Error::other)?;
            source.read_exact(&mut source_bytes[..length]).await?;
            destination
                .read_exact(&mut destination_bytes[..length])
                .await?;
            if source_bytes[..length] != destination_bytes[..length] {
                return Err(TransferError::Conflict);
            }
            remaining -= length as u64;
        }
        let mut written = prefix;
        loop {
            let length = source.read(&mut source_bytes).await?;
            if length == 0 {
                break;
            }
            written = written
                .checked_add(length as u64)
                .ok_or(TransferError::Conflict)?;
            if written > expected_size {
                return Err(TransferError::Conflict);
            }
            destination.write_all(&source_bytes[..length]).await?;
            destination.flush().await?;
            self.checkpoint(TransferCheckpointPoint::PublicationCopyChunkWritten)
                .await;
        }
        if written != expected_size {
            return Err(TransferError::Conflict);
        }
        destination.sync_all().await?;
        Ok(())
    }
}

async fn write_marker(
    marker: &TempArtifact,
    identity: [u8; 16],
    expected: ExpectedPublication,
) -> Result<(), TransferError> {
    let mut bytes = [0; MARKER_BYTES];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..24].copy_from_slice(&identity);
    bytes[24..32].copy_from_slice(&expected.size.to_be_bytes());
    bytes[32..].copy_from_slice(expected.sha256.as_bytes());
    let pending = marker.sibling("publication-copy.pending")?;
    let mut file = tokio::fs::File::from_std(pending.create_truncated()?.into_std());
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    drop(file);
    pending.rename_to(marker).await?;
    Ok(())
}

async fn read_marker(
    marker: &TempArtifact,
    expected: ExpectedPublication,
) -> Result<Option<[u8; 16]>, TransferError> {
    let Some((file, metadata)) = marker.open_read()? else {
        return Ok(None);
    };
    if metadata.len() != MARKER_BYTES as u64 {
        return Err(TransferError::Conflict);
    }
    let mut bytes = [0; MARKER_BYTES];
    tokio::fs::File::from_std(file.into_std())
        .read_exact(&mut bytes)
        .await?;
    if &bytes[..8] != MAGIC
        || bytes[24..32] != expected.size.to_be_bytes()
        || &bytes[32..] != expected.sha256.as_bytes()
    {
        return Err(TransferError::Conflict);
    }
    let mut identity = [0; 16];
    identity.copy_from_slice(&bytes[8..24]);
    Ok(Some(identity))
}
