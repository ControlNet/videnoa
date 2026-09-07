use std::io::Read;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::paths::{PathError, TempArtifact, TempWorkspace};
use crate::persistence::Sha256Digest;
use crate::remote::{FileApiPath, VidenoaClient};

use super::{HashingWriter, TransferError, VerifiedArtifact};

const EVIDENCE_BYTES: usize = 40;

pub(super) struct DownloadArtifact<'a> {
    pub client: &'a VidenoaClient,
    pub remote_path: &'a FileApiPath,
    pub workspace: TempWorkspace,
    pub extension: &'a str,
    pub expected_size: u64,
}

pub(super) enum VerifiedArtifactInspection {
    Missing,
    Valid(Box<VerifiedArtifact>),
    Invalid,
}

pub(super) async fn download_artifact(
    request: DownloadArtifact<'_>,
) -> Result<VerifiedArtifact, TransferError> {
    let part = request
        .workspace
        .artifact(format!("output.{}.part", request.extension))?;
    let verified = request
        .workspace
        .artifact(format!("output.{}.verified", request.extension))?;
    let evidence = request
        .workspace
        .artifact(format!("output.{}.verified.evidence", request.extension))?;
    let file = tokio::fs::File::from_std(part.create_truncated()?.into_std());
    let mut writer = HashingWriter::new(file);
    if let Err(error) = request
        .client
        .download(request.remote_path, &mut writer)
        .await
    {
        drop(writer);
        part.remove()?;
        return Err(error.into());
    }
    writer.flush().await?;
    let (file, size, sha256) = writer.finish();
    if size != request.expected_size {
        drop(file);
        part.remove()?;
        return Err(TransferError::Conflict);
    }
    file.sync_all().await?;
    write_evidence(&evidence, size, sha256).await?;
    install_verified(&part, &verified, size, sha256).await?;
    Ok(VerifiedArtifact {
        path: verified.display_path().to_path_buf(),
        size,
        sha256,
        source: verified,
    })
}

pub(super) async fn recover_verified(
    workspace: &TempWorkspace,
    extension: &str,
) -> Result<Option<VerifiedArtifact>, TransferError> {
    let verified = workspace.artifact(format!("output.{extension}.verified"))?;
    let evidence = workspace.artifact(format!("output.{extension}.verified.evidence"))?;
    let Some((file, metadata)) = verified.open_read()? else {
        evidence.remove()?;
        return Ok(None);
    };
    let Some((size, sha256)) = read_evidence(&evidence).await? else {
        remove_invalid_verified(&verified, &evidence).await?;
        return Ok(None);
    };
    let valid = size > 0 && metadata.len() == size && hash_file(file).await? == sha256;
    if !valid {
        remove_invalid_verified(&verified, &evidence).await?;
        return Ok(None);
    }
    Ok(Some(VerifiedArtifact {
        path: verified.display_path().to_path_buf(),
        size,
        sha256,
        source: verified,
    }))
}

pub(super) async fn inspect_verified(
    workspace: &TempWorkspace,
    extension: &str,
    expected_size: u64,
    expected_sha256: Sha256Digest,
) -> Result<VerifiedArtifactInspection, TransferError> {
    let verified = workspace.artifact(format!("output.{extension}.verified"))?;
    let (file, metadata) = match verified.open_read() {
        Ok(Some(artifact)) => artifact,
        Ok(None) => return Ok(VerifiedArtifactInspection::Missing),
        Err(PathError::InputNotRegular { .. } | PathError::SymlinkComponent { .. }) => {
            return Ok(VerifiedArtifactInspection::Invalid);
        }
        Err(error) => return Err(error.into()),
    };
    if metadata.len() != expected_size || hash_file(file).await? != expected_sha256 {
        return Ok(VerifiedArtifactInspection::Invalid);
    }
    Ok(VerifiedArtifactInspection::Valid(Box::new(VerifiedArtifact {
        path: verified.display_path().to_path_buf(),
        size: expected_size,
        sha256: expected_sha256,
        source: verified,
    })))
}

async fn remove_invalid_verified(
    verified: &TempArtifact,
    evidence: &TempArtifact,
) -> Result<(), TransferError> {
    verified.remove()?;
    evidence.remove()?;
    verified.sync_parent().await?;
    Ok(())
}

async fn install_verified(
    part: &TempArtifact,
    verified: &TempArtifact,
    size: u64,
    sha256: Sha256Digest,
) -> Result<(), TransferError> {
    if let Some((file, metadata)) = verified.open_read()? {
        let matches = metadata.len() == size && hash_file(file).await? == sha256;
        if matches {
            part.remove()?;
            part.sync_parent().await?;
            return Ok(());
        }
        verified.remove()?;
    }
    part.rename_to(verified).await?;
    Ok(())
}

async fn write_evidence(
    evidence: &TempArtifact,
    size: u64,
    sha256: Sha256Digest,
) -> Result<(), TransferError> {
    let mut bytes = [0_u8; EVIDENCE_BYTES];
    bytes[..8].copy_from_slice(&size.to_be_bytes());
    bytes[8..].copy_from_slice(sha256.as_bytes());
    let mut file = tokio::fs::File::from_std(evidence.create_truncated()?.into_std());
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    evidence.sync_parent().await?;
    Ok(())
}

async fn read_evidence(
    evidence: &TempArtifact,
) -> Result<Option<(u64, Sha256Digest)>, TransferError> {
    let Some((file, metadata)) = evidence.open_read()? else {
        return Ok(None);
    };
    if metadata.len() != EVIDENCE_BYTES as u64 {
        return Ok(None);
    }
    let mut bytes = [0_u8; EVIDENCE_BYTES];
    tokio::fs::File::from_std(file.into_std())
        .read_exact(&mut bytes)
        .await?;
    let mut size = [0_u8; 8];
    size.copy_from_slice(&bytes[..8]);
    let mut sha256 = [0_u8; 32];
    sha256.copy_from_slice(&bytes[8..]);
    Ok(Some((u64::from_be_bytes(size), Sha256Digest::new(sha256))))
}

async fn hash_file(file: cap_std::fs::File) -> Result<Sha256Digest, TransferError> {
    tokio::task::spawn_blocking(move || {
        let mut file = file;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok::<_, std::io::Error>(Sha256Digest::new(hasher.finalize().into()))
    })
    .await
    .map_err(std::io::Error::other)?
    .map_err(Into::into)
}
