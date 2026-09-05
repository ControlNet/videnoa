use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{File, OpenOptions};

use super::{identity, io_error, PathError, RootedOutput};

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
    pub(crate) fn open_copy(&self, owned: Option<[u8; 16]>) -> Result<File, PathError> {
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
        let metadata = file
            .metadata()
            .map_err(|source| io_error(&self.display_path, source))?;
        if !metadata.is_file()
            || owned.is_some_and(|expected| {
                Self::copy_identity(&file).map_or(true, |actual| actual != expected)
            })
        {
            return Err(PathError::OutputExists {
                path: self.display_path.clone(),
            });
        }
        Ok(file)
    }
}
