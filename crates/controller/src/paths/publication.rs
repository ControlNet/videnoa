use std::io;
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File, OpenOptions};

use super::{
    identity, publication_finalizer::sync_directory, select_root, PathCapabilities, PathError,
    PublicationFinalizer, RootedOutput, TempArtifact,
};

pub(crate) enum PublicationArtifact {
    Missing,
    Regular(File),
    NonRegular,
}

impl PathCapabilities {
    /// Reopens an output boundary that may contain durable publication artifacts.
    ///
    /// # Errors
    /// Returns a typed path error for escapes, symbolic components, or changed roots.
    pub fn reopen_output(&self, path: impl AsRef<Path>) -> Result<RootedOutput, PathError> {
        let path = self.media_path(path.as_ref(), &self.outputs)?;
        let path = path.as_path();
        let (root, relative) = select_root(&self.outputs, path)?;
        let leaf =
            relative
                .file_name()
                .map(PathBuf::from)
                .ok_or_else(|| PathError::InvalidPath {
                    path: path.to_path_buf(),
                })?;
        let requested_parent = relative.parent().unwrap_or_else(|| Path::new(""));
        let (directory, parent, missing_directories) =
            root.open_nearest_directory(requested_parent)?;
        let metadata = directory
            .dir_metadata()
            .map_err(|source| io_error(path, source))?;
        Ok(RootedOutput {
            root: root.clone(),
            parent,
            missing_directories,
            leaf,
            display_path: path.to_path_buf(),
            parent_identity: identity(&metadata),
            created_parent_identities: std::sync::Mutex::new(None),
        })
    }
}

impl RootedOutput {
    #[must_use]
    pub fn display_path(&self) -> &Path {
        &self.display_path
    }

    /// Opens the final output without following a symbolic link.
    ///
    /// # Errors
    /// Returns a typed path error when the boundary changed or inspection fails.
    pub(crate) fn open_final(&self) -> Result<PublicationArtifact, PathError> {
        self.open_existing(&self.leaf)
    }

    pub(crate) fn open_legacy_staging(&self, name: &str) -> Result<PublicationArtifact, PathError> {
        self.open_existing(&legacy_staging_leaf(name, &self.display_path)?)
    }

    pub(crate) fn prepare_publication(
        &self,
        source: &TempArtifact,
    ) -> Result<PublicationFinalizer, PathError> {
        let (source_directory, source_leaf, source_parent) = source.publication_source()?;
        Ok(PublicationFinalizer::new(
            source_directory,
            source_leaf,
            source_parent,
            self.open_parent(true)?,
            self.leaf.clone(),
            self.root.display_path().join(&self.parent),
        ))
    }

    pub(crate) fn sync_parent(&self) -> Result<(), PathError> {
        sync_directory(&self.open_parent(false)?)
            .map_err(|source| io_error(&self.display_path, source))
    }

    fn open_existing(&self, leaf: &Path) -> Result<PublicationArtifact, PathError> {
        self.open_existing_with(leaf, || Ok(()))
    }

    fn open_existing_with<F>(
        &self,
        leaf: &Path,
        checkpoint: F,
    ) -> Result<PublicationArtifact, PathError>
    where
        F: FnOnce() -> Result<(), io::Error>,
    {
        let directory = match self.open_parent(false) {
            Ok(directory) => directory,
            Err(PathError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(PublicationArtifact::Missing);
            }
            Err(error) => return Err(error),
        };
        checkpoint().map_err(|source| io_error(&self.display_path, source))?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        let display = self.root.display_path().join(&self.parent).join(leaf);
        match directory.open_with(leaf, &options) {
            Ok(file) => {
                let metadata = file
                    .metadata()
                    .map_err(|source| io_error(&display, source))?;
                if metadata.is_file() {
                    Ok(PublicationArtifact::Regular(file))
                } else {
                    Ok(PublicationArtifact::NonRegular)
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Ok(PublicationArtifact::Missing)
            }
            Err(source) => match directory.symlink_metadata(leaf) {
                Ok(metadata) if !metadata.is_file() => Ok(PublicationArtifact::NonRegular),
                Ok(_) | Err(_) => Err(io_error(&display, source)),
            },
        }
    }

    fn open_parent(&self, create_missing: bool) -> Result<Dir, PathError> {
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
        let expected_identities = self
            .created_parent_identities
            .lock()
            .map_err(|_| {
                io_error(
                    &self.display_path,
                    io::Error::other("output identity lock failed"),
                )
            })?
            .clone();
        let mut observed_identities = Vec::with_capacity(self.missing_directories.len());
        let mut traversed = self.root.display_path().join(&self.parent);
        for (index, component) in self.missing_directories.iter().enumerate() {
            traversed.push(component);
            match directory.symlink_metadata(component) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(PathError::SymlinkComponent {
                        path: traversed.clone(),
                    });
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound && create_missing => {
                    match directory.create_dir(component) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                        Err(source) => return Err(io_error(&traversed, source)),
                    }
                }
                Err(source) => return Err(io_error(&traversed, source)),
            }
            directory = directory
                .open_dir_nofollow(component)
                .map_err(|source| io_error(&traversed, source))?;
            let current_identity = identity(
                &directory
                    .dir_metadata()
                    .map_err(|source| io_error(&traversed, source))?,
            );
            if expected_identities
                .as_ref()
                .and_then(|identities| identities.get(index))
                .is_some_and(|expected| *expected != current_identity)
            {
                return Err(PathError::OutputParentChanged {
                    path: self.display_path.clone(),
                });
            }
            observed_identities.push(current_identity);
        }
        if create_missing && expected_identities.is_none() {
            *self.created_parent_identities.lock().map_err(|_| {
                io_error(
                    &self.display_path,
                    io::Error::other("output identity lock failed"),
                )
            })? = Some(observed_identities);
        }
        Ok(directory)
    }
}

fn legacy_staging_leaf(name: &str, output: &Path) -> Result<PathBuf, PathError> {
    let path = Path::new(name);
    let valid = name.starts_with(".videnoa-")
        && name.ends_with(".staging")
        && path.components().count() == 1
        && matches!(path.components().next(), Some(Component::Normal(_)));
    if !valid {
        return Err(PathError::InvalidPath {
            path: output.with_file_name(name),
        });
    }
    Ok(path.to_path_buf())
}

fn io_error(path: &Path, source: io::Error) -> PathError {
    PathError::Io {
        path: path.to_path_buf(),
        source,
    }
}
