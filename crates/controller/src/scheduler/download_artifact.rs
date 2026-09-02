use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::persistence::Sha256Digest;
use crate::remote::{FileApiPath, VidenoaClient};

use super::{HashingWriter, TransferError, VerifiedArtifact};

const EVIDENCE_BYTES: usize = 40;

pub(super) struct DownloadArtifact<'a> {
    pub client: &'a VidenoaClient,
    pub remote_path: &'a FileApiPath,
    pub directory: PathBuf,
    pub extension: &'a str,
    pub expected_size: u64,
}

pub(super) async fn download_artifact(
    request: DownloadArtifact<'_>,
) -> Result<VerifiedArtifact, TransferError> {
    tokio::fs::create_dir_all(&request.directory).await?;
    let part = request
        .directory
        .join(format!("output.{}.part", request.extension));
    let verified = request
        .directory
        .join(format!("output.{}.verified", request.extension));
    let evidence = evidence_path(&verified);
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&part)
        .await?;
    let mut writer = HashingWriter::new(file);
    if let Err(error) = request
        .client
        .download(request.remote_path, &mut writer)
        .await
    {
        drop(writer);
        remove_owned(&part).await?;
        return Err(error.into());
    }
    writer.flush().await?;
    let (file, size, sha256) = writer.finish();
    if size != request.expected_size {
        drop(file);
        remove_owned(&part).await?;
        return Err(TransferError::Conflict);
    }
    file.sync_all().await?;
    write_evidence(&evidence, size, sha256).await?;
    Box::pin(install_verified(&part, &verified, size, sha256)).await?;
    Ok(VerifiedArtifact {
        path: verified,
        size,
        sha256,
    })
}

pub(super) async fn recover_verified(
    directory: &Path,
    extension: &str,
) -> Result<Option<VerifiedArtifact>, TransferError> {
    let verified = directory.join(format!("output.{extension}.verified"));
    let evidence = evidence_path(&verified);
    let metadata = match tokio::fs::metadata(&verified).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            remove_owned(&evidence).await?;
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let Some((size, sha256)) = read_evidence(&evidence).await? else {
        remove_invalid_verified(&verified, &evidence).await?;
        return Ok(None);
    };
    let valid = metadata.is_file()
        && size > 0
        && metadata.len() == size
        && hash_file(&verified).await? == sha256;
    if !valid {
        remove_invalid_verified(&verified, &evidence).await?;
        return Ok(None);
    }
    Ok(Some(VerifiedArtifact {
        path: verified,
        size,
        sha256,
    }))
}

async fn remove_owned(path: &Path) -> Result<(), std::io::Error> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

async fn remove_invalid_verified(verified: &Path, evidence: &Path) -> Result<(), std::io::Error> {
    remove_owned(verified).await?;
    remove_owned(evidence).await?;
    sync_directory(parent(verified)?).await
}

async fn install_verified(
    part: &Path,
    verified: &Path,
    size: u64,
    sha256: Sha256Digest,
) -> Result<(), std::io::Error> {
    match tokio::fs::metadata(verified).await {
        Ok(metadata) => {
            let matches = metadata.len() == size && Box::pin(hash_file(verified)).await? == sha256;
            if matches {
                remove_owned(part).await?;
                return sync_directory(parent(verified)?).await;
            }
            remove_owned(verified).await?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    tokio::fs::rename(part, verified).await?;
    sync_directory(parent(verified)?).await
}

async fn write_evidence(
    path: &Path,
    size: u64,
    sha256: Sha256Digest,
) -> Result<(), std::io::Error> {
    let mut bytes = [0_u8; EVIDENCE_BYTES];
    bytes[..8].copy_from_slice(&size.to_be_bytes());
    bytes[8..].copy_from_slice(sha256.as_bytes());
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .await?;
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    sync_directory(parent(path)?).await
}

async fn read_evidence(path: &Path) -> Result<Option<(u64, Sha256Digest)>, std::io::Error> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() || metadata.len() != EVIDENCE_BYTES as u64 {
        return Ok(None);
    }
    let mut bytes = [0_u8; EVIDENCE_BYTES];
    tokio::fs::File::open(path)
        .await?
        .read_exact(&mut bytes)
        .await?;
    let mut size = [0_u8; 8];
    size.copy_from_slice(&bytes[..8]);
    let mut sha256 = [0_u8; 32];
    sha256.copy_from_slice(&bytes[8..]);
    Ok(Some((u64::from_be_bytes(size), Sha256Digest::new(sha256))))
}

fn evidence_path(verified: &Path) -> PathBuf {
    let mut path = verified.as_os_str().to_owned();
    path.push(".evidence");
    PathBuf::from(path)
}

async fn hash_file(path: &Path) -> Result<Sha256Digest, std::io::Error> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Sha256Digest::new(hasher.finalize().into()))
}

fn parent(path: &Path) -> Result<&Path, std::io::Error> {
    path.parent()
        .ok_or_else(|| std::io::Error::other("verified artifact has no parent directory"))
}

#[cfg(unix)]
async fn sync_directory(path: &Path) -> Result<(), std::io::Error> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || std::fs::File::open(path)?.sync_all())
        .await
        .map_err(std::io::Error::other)?
}

#[cfg(not(unix))]
async fn sync_directory(_path: &Path) -> Result<(), std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "durable directory synchronization is unsupported on this platform",
    ))
}
