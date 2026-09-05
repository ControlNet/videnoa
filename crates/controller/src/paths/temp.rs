use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, Metadata, OpenOptions};

use crate::domain::TaskId;

use super::{identity, Identity, PathCapabilities, PathError, Root};

#[derive(Clone)]
pub(crate) struct TempWorkspace {
    root: Root,
    directory: Arc<Dir>,
    identity: Identity,
    leaf: PathBuf,
    display_path: PathBuf,
}

#[derive(Clone)]
pub(crate) struct TempArtifact {
    workspace: TempWorkspace,
    leaf: PathBuf,
    display_path: PathBuf,
}

impl PathCapabilities {
    pub(crate) fn temp_workspace(
        &self,
        task_id: TaskId,
        create: bool,
    ) -> Result<Option<TempWorkspace>, PathError> {
        self.temp.ensure_current()?;
        let leaf = PathBuf::from(task_id.to_string());
        let root_directory = self.temp.clone_directory()?;
        let display_path = self.temp.display_path().join(&leaf);
        match root_directory.symlink_metadata(&leaf) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(PathError::SymlinkComponent { path: display_path });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
                root_directory
                    .create_dir(&leaf)
                    .map_err(|source| io_error(&display_path, source))?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(io_error(&display_path, source)),
        }
        let directory = root_directory
            .open_dir_nofollow(&leaf)
            .map_err(|source| io_error(&display_path, source))?;
        let metadata = directory
            .dir_metadata()
            .map_err(|source| io_error(&display_path, source))?;
        Ok(Some(TempWorkspace {
            root: self.temp.clone(),
            directory: Arc::new(directory),
            identity: identity(&metadata),
            leaf,
            display_path,
        }))
    }
}

impl TempWorkspace {
    pub(crate) fn artifact(&self, leaf: impl AsRef<Path>) -> Result<TempArtifact, PathError> {
        let leaf = leaf.as_ref();
        if leaf.components().count() != 1
            || !matches!(leaf.components().next(), Some(Component::Normal(_)))
        {
            return Err(PathError::InvalidPath {
                path: self.display_path.join(leaf),
            });
        }
        Ok(TempArtifact {
            workspace: self.clone(),
            leaf: leaf.to_path_buf(),
            display_path: self.display_path.join(leaf),
        })
    }

    pub(crate) async fn remove_all(&self) -> Result<(), PathError> {
        self.current_directory()?;
        let root = self.root.clone_directory()?;
        let sync_root = root
            .try_clone()
            .map_err(|source| io_error(&self.display_path, source))?;
        let leaf = self.leaf.clone();
        let display = self.display_path.clone();
        tokio::task::spawn_blocking(move || root.remove_dir_all(&leaf))
            .await
            .map_err(|source| io_error(&display, io::Error::other(source)))?
            .map_err(|source| io_error(&display, source))?;
        sync_directory(sync_root, self.root.display_path().to_path_buf()).await
    }

    fn current_directory(&self) -> Result<Dir, PathError> {
        self.root.ensure_current()?;
        let root = self.root.clone_directory()?;
        let current = root
            .open_dir_nofollow(&self.leaf)
            .map_err(|source| io_error(&self.display_path, source))?;
        let metadata = current
            .dir_metadata()
            .map_err(|source| io_error(&self.display_path, source))?;
        if identity(&metadata) != self.identity {
            return Err(PathError::RootChanged {
                path: self.display_path.clone(),
            });
        }
        self.directory
            .try_clone()
            .map_err(|source| io_error(&self.display_path, source))
    }
}

impl TempArtifact {
    pub(crate) fn display_path(&self) -> &Path {
        &self.display_path
    }

    pub(super) fn publication_source(&self) -> Result<(Dir, PathBuf, PathBuf), PathError> {
        Ok((
            self.workspace.current_directory()?,
            self.leaf.clone(),
            self.workspace.display_path.clone(),
        ))
    }

    pub(crate) fn open_read(&self) -> Result<Option<(cap_std::fs::File, Metadata)>, PathError> {
        let directory = self.workspace.current_directory()?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No).nonblock(true);
        let file = match directory.open_with(&self.leaf, &options) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(io_error(&self.display_path, source)),
        };
        let metadata = file
            .metadata()
            .map_err(|source| io_error(&self.display_path, source))?;
        if !metadata.is_file() {
            return Err(PathError::InputNotRegular {
                path: self.display_path.clone(),
            });
        }
        Ok(Some((file, metadata)))
    }

    pub(crate) fn create_truncated(&self) -> Result<cap_std::fs::File, PathError> {
        let directory = self.workspace.current_directory()?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create(true)
            .truncate(true)
            .follow(FollowSymlinks::No)
            .nonblock(true);
        let file = directory
            .open_with(&self.leaf, &options)
            .map_err(|source| io_error(&self.display_path, source))?;
        let metadata = file
            .metadata()
            .map_err(|source| io_error(&self.display_path, source))?;
        if !metadata.is_file() {
            return Err(PathError::InputNotRegular {
                path: self.display_path.clone(),
            });
        }
        Ok(file)
    }

    pub(crate) fn remove(&self) -> Result<(), PathError> {
        let directory = self.workspace.current_directory()?;
        let metadata = match directory.symlink_metadata(&self.leaf) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => return Err(io_error(&self.display_path, source)),
        };
        if metadata.file_type().is_symlink() {
            return Err(PathError::SymlinkComponent {
                path: self.display_path.clone(),
            });
        }
        if !metadata.is_file() {
            return Err(PathError::InputNotRegular {
                path: self.display_path.clone(),
            });
        }
        match directory.remove_file(&self.leaf) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(io_error(&self.display_path, source)),
        }
    }

    pub(crate) async fn rename_to(&self, destination: &Self) -> Result<(), PathError> {
        let directory = self.workspace.current_directory()?;
        directory
            .rename(&self.leaf, &directory, &destination.leaf)
            .map_err(|source| io_error(&self.display_path, source))?;
        sync_directory(directory, self.workspace.display_path.clone()).await
    }

    pub(crate) async fn sync_parent(&self) -> Result<(), PathError> {
        sync_directory(
            self.workspace.current_directory()?,
            self.workspace.display_path.clone(),
        )
        .await
    }
}

async fn sync_directory(directory: Dir, display: PathBuf) -> Result<(), PathError> {
    tokio::task::spawn_blocking(move || sync_directory_blocking(&directory))
        .await
        .map_err(|source| io_error(&display, io::Error::other(source)))?
        .map_err(|source| io_error(&display, source))
}

#[cfg(unix)]
fn sync_directory_blocking(directory: &Dir) -> Result<(), io::Error> {
    let sync_handle = rustix::fs::openat(
        directory,
        ".",
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(io::Error::from)?;
    rustix::fs::fsync(sync_handle).map_err(io::Error::from)
}

#[cfg(windows)]
fn sync_directory_blocking(directory: &Dir) -> Result<(), io::Error> {
    directory.try_clone()?.into_std_file().sync_all()
}

#[cfg(not(any(unix, windows)))]
fn sync_directory_blocking(_directory: &Dir) -> Result<(), io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "durable directory synchronization is unsupported on this platform",
    ))
}

fn io_error(path: &Path, source: io::Error) -> PathError {
    PathError::Io {
        path: path.to_path_buf(),
        source,
    }
}
