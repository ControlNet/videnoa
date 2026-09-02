use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{File, OpenOptions};

use crate::config::PathConfig;

mod root;
use root::{identity, select_root, Root};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("path is not an unambiguous absolute path: {path}")]
    InvalidPath { path: PathBuf },
    #[error("path is outside configured roots: {path}")]
    OutsideRoots { path: PathBuf },
    #[error("path contains a symbolic-link component: {path}")]
    SymlinkComponent { path: PathBuf },
    #[error("configured root changed after capabilities were opened: {path}")]
    RootChanged { path: PathBuf },
    #[error("input is not a regular file: {path}")]
    InputNotRegular { path: PathBuf },
    #[error("input changed after it was accepted: {path}")]
    InputChanged { path: PathBuf },
    #[error("output already exists: {path}")]
    OutputExists { path: PathBuf },
    #[error("output parent changed after it was accepted: {path}")]
    OutputParentChanged { path: PathBuf },
    #[error("filesystem access failed for {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Clone)]
pub struct PathCapabilities {
    inputs: Vec<Root>,
    outputs: Vec<Root>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Identity {
    device: u64,
    inode: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputSnapshot {
    identity: Identity,
    pub length: u64,
    pub modified: SystemTime,
}

pub struct RootedInput {
    root: Root,
    relative: PathBuf,
    display_path: PathBuf,
    snapshot: InputSnapshot,
}

pub struct RootedOutput {
    root: Root,
    parent: PathBuf,
    missing_directories: Vec<PathBuf>,
    leaf: PathBuf,
    display_path: PathBuf,
    parent_identity: Identity,
}

impl InputSnapshot {
    #[must_use]
    pub fn platform_identity(&self) -> [u8; 16] {
        let mut bytes = [0_u8; 16];
        bytes[..8].copy_from_slice(&self.identity.device.to_le_bytes());
        bytes[8..].copy_from_slice(&self.identity.inode.to_le_bytes());
        bytes
    }
}

impl PathCapabilities {
    /// Opens and retains descriptor-backed input and output roots.
    ///
    /// # Errors
    /// Returns a typed path error when a root is missing, replaced, or symbolic.
    pub fn open(config: &PathConfig) -> Result<Self, PathError> {
        Ok(Self {
            inputs: config
                .input_roots
                .iter()
                .map(|path| Root::open(path))
                .collect::<Result<_, _>>()?,
            outputs: config
                .output_roots
                .iter()
                .map(|path| Root::open(path))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Opens a regular input and records its descriptor-derived identity and metadata.
    ///
    /// # Errors
    /// Returns a typed path error when the input escapes a root or is not a regular file.
    pub fn open_input(&self, path: impl AsRef<Path>) -> Result<RootedInput, PathError> {
        let path = path.as_ref();
        let (root, relative) = select_root(&self.inputs, path)?;
        let file = root.open_file(&relative, false)?;
        let metadata = file.metadata().map_err(|source| io_error(path, source))?;
        if !metadata.is_file() {
            return Err(PathError::InputNotRegular {
                path: path.to_path_buf(),
            });
        }
        let snapshot = InputSnapshot {
            identity: identity(&metadata),
            length: metadata.len(),
            modified: metadata
                .modified()
                .map_err(|source| io_error(path, source))?
                .into_std(),
        };
        Ok(RootedInput {
            root: root.clone(),
            relative,
            display_path: path.to_path_buf(),
            snapshot,
        })
    }

    /// Maps a non-existing output below the nearest existing rooted parent.
    ///
    /// # Errors
    /// Returns a typed path error for escapes, symbolic components, or collisions.
    pub fn open_output(&self, path: impl AsRef<Path>) -> Result<RootedOutput, PathError> {
        let path = path.as_ref();
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
        if missing_directories.is_empty() {
            match directory.symlink_metadata(&leaf) {
                Ok(_) => {
                    return Err(PathError::OutputExists {
                        path: path.to_path_buf(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(io_error(path, source)),
            }
        }
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
        })
    }
}

impl RootedInput {
    #[must_use]
    pub const fn snapshot(&self) -> &InputSnapshot {
        &self.snapshot
    }

    /// Reopens the input and requires identity, size, and modification time to match.
    ///
    /// # Errors
    /// Returns [`PathError::InputChanged`] when the accepted input was replaced or modified.
    pub fn reopen_checked(&self) -> Result<File, PathError> {
        self.root.ensure_current()?;
        let file = self.root.open_file(&self.relative, true)?;
        let metadata = file
            .metadata()
            .map_err(|source| io_error(&self.display_path, source))?;
        let modified = metadata
            .modified()
            .map_err(|source| io_error(&self.display_path, source))?
            .into_std();
        if !metadata.is_file()
            || identity(&metadata) != self.snapshot.identity
            || metadata.len() != self.snapshot.length
            || modified != self.snapshot.modified
        {
            return Err(PathError::InputChanged {
                path: self.display_path.clone(),
            });
        }
        Ok(file)
    }
}

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

fn io_error(path: &Path, source: io::Error) -> PathError {
    PathError::Io {
        path: path.to_path_buf(),
        source,
    }
}
