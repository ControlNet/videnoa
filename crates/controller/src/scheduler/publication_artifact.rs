use cap_std::fs::File;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

use crate::persistence::Sha256Digest;

use super::TransferError;

const COPY_BUFFER_BYTES: usize = 64 * 1024;

pub(super) async fn matches_file(
    file: File,
    expected_size: u64,
    expected_sha256: Sha256Digest,
) -> Result<bool, TransferError> {
    let (size, sha256) = tokio::task::spawn_blocking(move || hash_reader(file))
        .await
        .map_err(std::io::Error::other)??;
    Ok(size == expected_size && sha256 == expected_sha256)
}

pub(super) async fn copy_verified(
    source: File,
    staging: File,
    expected_size: u64,
    expected_sha256: Sha256Digest,
) -> Result<(), TransferError> {
    tokio::task::spawn_blocking(move || {
        let mut source = source;
        let mut staging = staging;
        let mut hasher = Sha256::new();
        let mut size = 0_u64;
        let mut buffer = vec![0_u8; COPY_BUFFER_BYTES].into_boxed_slice();
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            size = size
                .checked_add(u64::try_from(read).map_err(std::io::Error::other)?)
                .ok_or_else(|| std::io::Error::other("publication size overflow"))?;
            if size > expected_size {
                return Err(std::io::Error::other(
                    "verified artifact exceeds durable publication length",
                ));
            }
            hasher.update(&buffer[..read]);
            staging.write_all(&buffer[..read])?;
        }
        staging.flush()?;
        staging.sync_all()?;
        let sha256 = Sha256Digest::new(hasher.finalize().into());
        if size != expected_size || sha256 != expected_sha256 {
            return Err(std::io::Error::other(
                "verified artifact does not match durable publication evidence",
            ));
        }
        Ok(())
    })
    .await
    .map_err(std::io::Error::other)??;
    Ok(())
}

fn hash_reader(mut file: File) -> Result<(u64, Sha256Digest), std::io::Error> {
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES].into_boxed_slice();
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size = size
            .checked_add(u64::try_from(read).map_err(std::io::Error::other)?)
            .ok_or_else(|| std::io::Error::other("publication size overflow"))?;
        hasher.update(&buffer[..read]);
    }
    Ok((size, Sha256Digest::new(hasher.finalize().into())))
}
