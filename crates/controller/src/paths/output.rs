use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{File, OpenOptions};
use std::io;

use super::{identity, io_error, PathError, RootedOutput};

impl RootedOutput {
    /// Revalidates root identity, no-follow parents, and final-leaf absence without creating it.
    ///
    /// # Errors
    /// Returns a typed path error when the accepted output boundary drifted.
    pub fn revalidate_missing(&self) -> Result<(), PathError> {
        self.root.ensure_current()?;
        let mut directory = self.root.open_directory(&self.parent)?;
        let metadata = directory
            .dir_metadata()
            .map_err(|source| io_error(&self.display_path, source))?;
        if identity(&metadata) != self.parent_identity {
            return Err(PathError::OutputParentChanged {
                path: self.display_path.clone(),
            });
        }
        let mut traversed = self.root.display_path().join(&self.parent);
        for component in &self.missing_directories {
            traversed.push(component);
            match directory.symlink_metadata(component) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(PathError::SymlinkComponent {
                        path: traversed.clone(),
                    });
                }
                Ok(_) => {
                    directory = directory
                        .open_dir_nofollow(component)
                        .map_err(|source| io_error(&traversed, source))?;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(source) => return Err(io_error(&traversed, source)),
            }
        }
        match directory.symlink_metadata(&self.leaf) {
            Ok(_) => Err(PathError::OutputExists {
                path: self.display_path.clone(),
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(io_error(&self.display_path, source)),
        }
    }

    /// Creates missing rooted parents and the output leaf with no-clobber semantics.
    ///
    /// # Errors
    /// Returns a typed path error when a parent changed, a symlink appears, or the leaf exists.
    pub fn create_new(&self) -> Result<File, PathError> {
        self.root.ensure_current()?;
        let mut directory = self.root.open_directory(&self.parent)?;
        let metadata = directory
            .dir_metadata()
            .map_err(|source| io_error(&self.display_path, source))?;
        if identity(&metadata) != self.parent_identity {
            return Err(PathError::OutputParentChanged {
                path: self.display_path.clone(),
            });
        }
        let mut traversed = self.root.display_path().join(&self.parent);
        for component in &self.missing_directories {
            traversed.push(component);
            match directory.create_dir(component) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let metadata = directory
                        .symlink_metadata(component)
                        .map_err(|source| io_error(&traversed, source))?;
                    if metadata.file_type().is_symlink() {
                        return Err(PathError::SymlinkComponent {
                            path: traversed.clone(),
                        });
                    }
                }
                Err(source) => return Err(io_error(&traversed, source)),
            }
            directory = directory
                .open_dir_nofollow(component)
                .map_err(|source| io_error(&traversed, source))?;
        }
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        directory.open_with(&self.leaf, &options).map_err(|source| {
            if source.kind() == io::ErrorKind::AlreadyExists {
                PathError::OutputExists {
                    path: self.display_path.clone(),
                }
            } else {
                io_error(&self.display_path, source)
            }
        })
    }
}
