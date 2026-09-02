use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::persistence::Sha256Digest;
use crate::remote::{FileApiPath, VidenoaClient};

use super::{HashingWriter, TransferError, VerifiedArtifact};

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
        remove_partial(&part).await?;
        return Err(error.into());
    }
    writer.flush().await?;
    let (file, size, sha256) = writer.finish();
    if size != request.expected_size {
        drop(file);
        remove_partial(&part).await?;
        return Err(TransferError::Conflict);
    }
    file.sync_all().await?;
    Box::pin(install_verified(&part, &verified, size, sha256)).await?;
    Ok(VerifiedArtifact {
        path: verified,
        size,
        sha256,
    })
}

async fn remove_partial(path: &Path) -> Result<(), std::io::Error> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
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
                tokio::fs::remove_file(part).await?;
                return sync_directory(parent(verified)?).await;
            }
            tokio::fs::remove_file(verified).await?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    tokio::fs::rename(part, verified).await?;
    sync_directory(parent(verified)?).await
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
