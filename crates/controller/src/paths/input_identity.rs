use std::io::{Read, Seek};
use std::path::Path;

use cap_std::fs::File;
use sha2::{Digest, Sha256};

use super::{io_error, PathError};

pub(super) fn content_identity(file: &mut File, path: &Path) -> Result<[u8; 16], PathError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| io_error(path, source))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    file.rewind().map_err(|source| io_error(path, source))?;
    let digest = hasher.finalize();
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&digest[..16]);
    Ok(identity)
}
