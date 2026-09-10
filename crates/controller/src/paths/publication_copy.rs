use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File, OpenOptions};
use std::{io, path::PathBuf};

use super::{identity, io_error, publication_finalizer::sync_directory, PathError, RootedOutput};

/// Retains the directory and file handles needed to roll back a newly created empty output.
pub(crate) struct CopyDestination {
    directory: Dir,
    leaf: PathBuf,
    file: File,
    remove_empty_on_drop: bool,
}

impl CopyDestination {
    pub(crate) fn file(&self) -> &File {
        &self.file
    }

    pub(crate) fn sync_parent(&self) -> io::Result<()> {
        sync_directory(&self.directory)
    }

    pub(crate) fn validate_visible(&self, output: &RootedOutput) -> Result<(), PathError> {
        let super::PublicationArtifact::Regular(file) = output.open_copy_final()? else {
            return Err(PathError::OutputParentChanged {
                path: output.display_path.clone(),
            });
        };
        let current = RootedOutput::copy_identity(&file)
            .map_err(|source| io_error(&output.display_path, source))?;
        let owned = RootedOutput::copy_identity(&self.file)
            .map_err(|source| io_error(&output.display_path, source))?;
        if current != owned {
            return Err(PathError::OutputParentChanged {
                path: output.display_path.clone(),
            });
        }
        Ok(())
    }

    pub(crate) fn keep(&mut self) {
        self.remove_empty_on_drop = false;
    }

    fn remove_empty(&self) -> io::Result<()> {
        let owned = self.file.metadata()?;
        if !owned.is_file() || owned.len() != 0 {
            return Ok(());
        }
        let current = match self.directory.symlink_metadata(&self.leaf) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        // Never follow a replacement link or remove a different or nonempty file.
        if !current.is_file() || current.len() != 0 || identity(&current) != identity(&owned) {
            return Ok(());
        }
        self.directory.remove_file(&self.leaf)?;
        sync_directory(&self.directory)
    }
}

impl Drop for CopyDestination {
    fn drop(&mut self) {
        if self.remove_empty_on_drop {
            if let Err(error) = self.remove_empty() {
                // Preserve the original stage failure; cleanup diagnostics never expose paths.
                tracing::warn!(operation = "copy.cleanup_empty_destination",
                    io_kind = ?error.kind(), raw_os_error = ?error.raw_os_error(),
                    "Could not durably remove empty publication output");
            }
        }
    }
}

impl RootedOutput {
    pub(crate) fn copy_identity(file: &File) -> Result<[u8; 16], std::io::Error> {
        let identity = identity(&file.metadata()?);
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&identity.device.to_le_bytes());
        bytes[8..].copy_from_slice(&identity.inode.to_le_bytes());
        Ok(bytes)
    }

    // Existing output may only be reopened for append after matching durable ownership.
    // Neither branch truncates or follows a symbolic link.
    pub(crate) fn open_copy(&self, owned: Option<[u8; 16]>) -> Result<CopyDestination, PathError> {
        let directory = self.open_parent(owned.is_none())?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .append(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        if owned.is_none() {
            options.create_new(true);
        }
        let file = directory
            .open_with(&self.leaf, &options)
            .map_err(|source| io_error(&self.display_path, source))?;
        let destination = CopyDestination {
            directory,
            leaf: self.leaf.clone(),
            file,
            remove_empty_on_drop: owned.is_none(),
        };
        let metadata = destination
            .file
            .metadata()
            .map_err(|source| io_error(&self.display_path, source))?;
        if !metadata.is_file()
            || owned.is_some_and(|expected| {
                Self::copy_identity(&destination.file).map_or(true, |actual| actual != expected)
            })
        {
            return Err(PathError::OutputExists {
                path: self.display_path.clone(),
            });
        }
        Ok(destination)
    }
}
