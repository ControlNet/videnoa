use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::config::PathConfig;

mod boundary;
mod input;
mod input_identity;
mod output;
mod publication;
mod publication_finalizer;
#[cfg(test)]
mod publication_tests;
mod root;
mod temp;
use input_identity::content_identity;
pub(crate) use publication::PublicationArtifact;
pub(crate) use publication_finalizer::PublicationFinalizer;
use root::{identity, select_root, Root};
pub(crate) use temp::{TempArtifact, TempWorkspace};

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
    #[error("atomic publication cannot cross filesystems from {source_path} to {destination}")]
    CrossFilesystemPublication {
        source_path: PathBuf,
        destination: PathBuf,
    },
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
    data: Root,
    temp: Root,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Identity {
    device: u64,
    inode: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputSnapshot {
    identity: Identity,
    content_identity: [u8; 16],
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
    created_parent_identities: Mutex<Option<Vec<Identity>>>,
}

impl InputSnapshot {
    #[must_use]
    pub fn platform_identity(&self) -> [u8; 16] {
        let mut bytes = [0_u8; 16];
        bytes[..8].copy_from_slice(&self.identity.device.to_le_bytes());
        bytes[8..].copy_from_slice(&self.identity.inode.to_le_bytes());
        bytes
    }

    #[must_use]
    pub const fn content_identity(&self) -> [u8; 16] {
        self.content_identity
    }
}

impl PathCapabilities {
    /// Opens and retains descriptor-backed input and output roots.
    ///
    /// # Errors
    /// Returns a typed path error when a root is missing, replaced, or symbolic.
    pub fn open(config: &PathConfig) -> Result<Self, PathError> {
        let inputs = config
            .input_roots
            .iter()
            .map(|path| Root::open(path))
            .collect::<Result<_, _>>()?;
        let outputs: Vec<Root> = config
            .output_roots
            .iter()
            .map(|path| Root::open(path))
            .collect::<Result<_, _>>()?;
        let temp = Root::open(&config.temp_root)?;
        let data = Root::open(&config.data_root)?;
        if let Some(output) = outputs
            .iter()
            .find(|output| output.device() != temp.device())
        {
            return Err(PathError::CrossFilesystemPublication {
                source_path: temp.display_path().to_path_buf(),
                destination: output.display_path().to_path_buf(),
            });
        }
        Ok(Self {
            inputs,
            outputs,
            data,
            temp,
        })
    }

    /// # Errors
    /// Returns a path error when a retained root capability is no longer current.
    pub fn check_ready(&self) -> Result<(), PathError> {
        self.inputs
            .iter()
            .chain(&self.outputs)
            .try_for_each(Root::ensure_current)?;
        self.data.ensure_current()?;
        self.temp.ensure_current()
    }

    /// Opens a regular input and records its descriptor-derived identity and metadata.
    ///
    /// # Errors
    /// Returns a typed path error when the input escapes a root or is not a regular file.
    pub fn open_input(&self, path: impl AsRef<Path>) -> Result<RootedInput, PathError> {
        let path = self.media_path(path.as_ref(), &self.inputs)?;
        let path = path.as_path();
        let (root, relative) = select_root(&self.inputs, path)?;
        let mut file = root.open_file(&relative, false)?;
        let metadata = file.metadata().map_err(|source| io_error(path, source))?;
        if !metadata.is_file() {
            return Err(PathError::InputNotRegular {
                path: path.to_path_buf(),
            });
        }
        let accepted_identity = identity(&metadata);
        let accepted_length = metadata.len();
        let accepted_modified = metadata
            .modified()
            .map_err(|source| io_error(path, source))?
            .into_std();
        let content_identity = content_identity(&mut file, path)?;
        let current = file.metadata().map_err(|source| io_error(path, source))?;
        let current_modified = current
            .modified()
            .map_err(|source| io_error(path, source))?
            .into_std();
        if identity(&current) != accepted_identity
            || current.len() != accepted_length
            || current_modified != accepted_modified
        {
            return Err(PathError::InputChanged {
                path: path.to_path_buf(),
            });
        }
        let snapshot = InputSnapshot {
            identity: accepted_identity,
            content_identity,
            length: accepted_length,
            modified: accepted_modified,
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
            created_parent_identities: Mutex::new(None),
        })
    }
}

fn io_error(path: &Path, source: io::Error) -> PathError {
    PathError::Io {
        path: path.to_path_buf(),
        source,
    }
}
