use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{ensure, Context, Result};
use iroh::SecretKey;

/// A persistent identity and exclusive ownership of its runtime directory.
/// Private key material deliberately has no Debug/Serialize implementation.
pub struct Identity {
    pub(crate) key: SecretKey,
    _lock: File,
}

impl Identity {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let path = root.join("iroh.key");
        let lock_path = root.join("iroh.lock");
        reject_symlink(&lock_path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options.open(lock_path)?;
        lock.try_lock().context("iroh identity is already in use")?;
        reject_symlink(&path)?;
        let mut reader = OpenOptions::new();
        reader.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            reader.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let key = match reader.open(&path) {
            Ok(file) => {
                ensure!(
                    file.metadata()?.is_file(),
                    "iroh identity is not a regular file"
                );
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
                }
                let mut bytes = Vec::new();
                file.take(33).read_to_end(&mut bytes)?;
                let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
                    anyhow::anyhow!(
                        "invalid iroh identity length; restore the original identity file"
                    )
                })?;
                SecretKey::from_bytes(&bytes)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let key = SecretKey::generate();
                let mut temp = tempfile::NamedTempFile::new_in(root)?;
                temp.write_all(&key.to_bytes())?;
                temp.as_file().sync_all()?;
                temp.persist_noclobber(&path)
                    .context("cannot persist iroh identity")?;
                #[cfg(unix)]
                File::open(root)?.sync_all()?;
                key
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self { key, _lock: lock })
    }

    pub fn id(&self) -> iroh::EndpointId {
        self.key.public()
    }
}

fn reject_symlink(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "iroh identity storage must be a regular file"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_survives_restart_and_excludes_concurrent_owners() -> Result<()> {
        let root = tempfile::tempdir()?;
        let first = Identity::open(root.path())?;
        let id = first.id();
        assert!(Identity::open(root.path()).is_err());
        drop(first);
        assert_eq!(Identity::open(root.path())?.id(), id);
        Ok(())
    }

    #[test]
    fn corrupt_identity_is_not_replaced() -> Result<()> {
        let root = tempfile::tempdir()?;
        // Deliberately invalid test-only identity bytes.
        std::fs::write(root.path().join("iroh.key"), b"invalid")?;
        assert!(Identity::open(root.path()).is_err());
        assert_eq!(std::fs::read(root.path().join("iroh.key"))?, b"invalid");
        Ok(())
    }
}
