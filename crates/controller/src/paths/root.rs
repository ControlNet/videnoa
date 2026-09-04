use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use cap_fs_ext::{ambient_authority, DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};

use super::{Identity, PathError};

#[derive(Clone)]
pub(super) struct Root {
    ambient_path: PathBuf,
    directory: Arc<Dir>,
    identity: Identity,
}

impl Root {
    pub(super) fn display_path(&self) -> &Path {
        &self.ambient_path
    }

    pub(super) fn open(path: &Path) -> Result<Self, PathError> {
        let ambient_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|source| io_error(path, source))?
                .join(path)
        };
        let directory = open_absolute_directory(&ambient_path)?;
        let metadata = directory
            .dir_metadata()
            .map_err(|source| io_error(&ambient_path, source))?;
        Ok(Self {
            ambient_path,
            directory: Arc::new(directory),
            identity: identity(&metadata),
        })
    }

    pub(super) fn ensure_current(&self) -> Result<(), PathError> {
        let changed = || PathError::RootChanged {
            path: self.ambient_path.clone(),
        };
        let current = open_absolute_directory(&self.ambient_path).map_err(|_| changed())?;
        let metadata = current.dir_metadata().map_err(|_| changed())?;
        if identity(&metadata) != self.identity {
            return Err(changed());
        }
        Ok(())
    }

    pub(super) fn open_directory(&self, relative: &Path) -> Result<Dir, PathError> {
        let mut directory = self
            .directory
            .try_clone()
            .map_err(|source| io_error(&self.ambient_path, source))?;
        let mut traversed = self.ambient_path.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(PathError::InvalidPath { path: traversed });
            };
            traversed.push(name);
            let metadata = directory
                .symlink_metadata(name)
                .map_err(|source| io_error(&traversed, source))?;
            if metadata.file_type().is_symlink() {
                return Err(PathError::SymlinkComponent { path: traversed });
            }
            directory = directory
                .open_dir_nofollow(name)
                .map_err(|source| io_error(&traversed, source))?;
        }
        Ok(directory)
    }

    pub(super) fn clone_directory(&self) -> Result<Dir, PathError> {
        self.directory
            .try_clone()
            .map_err(|source| io_error(&self.ambient_path, source))
    }

    pub(super) fn open_nearest_directory(
        &self,
        relative: &Path,
    ) -> Result<(Dir, PathBuf, Vec<PathBuf>), PathError> {
        let mut directory = self
            .directory
            .try_clone()
            .map_err(|source| io_error(&self.ambient_path, source))?;
        let mut existing = PathBuf::new();
        let mut missing = Vec::new();
        let mut traversed = self.ambient_path.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(PathError::InvalidPath { path: traversed });
            };
            traversed.push(name);
            if !missing.is_empty() {
                missing.push(PathBuf::from(name));
                continue;
            }
            match directory.symlink_metadata(name) {
                Ok(metadata) => {
                    if metadata.file_type().is_symlink() {
                        return Err(PathError::SymlinkComponent { path: traversed });
                    }
                    directory = directory
                        .open_dir_nofollow(name)
                        .map_err(|source| io_error(&traversed, source))?;
                    existing.push(name);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    missing.push(PathBuf::from(name));
                }
                Err(source) => return Err(io_error(&traversed, source)),
            }
        }
        Ok((directory, existing, missing))
    }

    pub(super) fn open_file(
        &self,
        relative: &Path,
        changed_is_distinct: bool,
    ) -> Result<File, PathError> {
        let parent = relative.parent().unwrap_or_else(|| Path::new(""));
        let leaf = relative.file_name().ok_or_else(|| PathError::InvalidPath {
            path: self.ambient_path.join(relative),
        })?;
        let directory = self.open_directory(parent)?;
        let full_path = self.ambient_path.join(relative);
        match directory.symlink_metadata(leaf) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(PathError::SymlinkComponent { path: full_path });
            }
            Ok(_) => {}
            Err(source) if changed_is_distinct && source.kind() == io::ErrorKind::NotFound => {
                return Err(PathError::InputChanged { path: full_path });
            }
            Err(source) => return Err(io_error(&full_path, source)),
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        directory
            .open_with(leaf, &options)
            .map_err(|source| io_error(&full_path, source))
    }
}

pub(super) fn select_root<'a>(
    roots: &'a [Root],
    path: &Path,
) -> Result<(&'a Root, PathBuf), PathError> {
    validate_absolute(path)?;
    for root in roots {
        if let Ok(relative) = path.strip_prefix(&root.ambient_path) {
            validate_relative(relative, path)?;
            root.ensure_current()?;
            return Ok((root, relative.to_path_buf()));
        }
    }
    Err(PathError::OutsideRoots {
        path: path.to_path_buf(),
    })
}

fn validate_absolute(path: &Path) -> Result<(), PathError> {
    #[cfg(not(windows))]
    let malformed = path.to_string_lossy().contains('\\');
    #[cfg(windows)]
    let malformed = false;
    if !path.is_absolute() || malformed {
        return Err(PathError::InvalidPath {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn validate_relative(relative: &Path, original: &Path) -> Result<(), PathError> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PathError::InvalidPath {
            path: original.to_path_buf(),
        });
    }
    Ok(())
}

fn open_absolute_directory(path: &Path) -> Result<Dir, PathError> {
    validate_absolute(path)?;
    let mut anchor = PathBuf::new();
    let mut directory = None;
    let mut traversed = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir if directory.is_none() => {
                anchor.push(component.as_os_str());
                traversed.push(component.as_os_str());
            }
            Component::Normal(name) => {
                let parent = match directory.take() {
                    Some(parent) => parent,
                    None => Dir::open_ambient_dir(&anchor, ambient_authority())
                        .map_err(|source| io_error(&anchor, source))?,
                };
                traversed.push(name);
                let metadata = parent
                    .symlink_metadata(name)
                    .map_err(|source| io_error(&traversed, source))?;
                if metadata.file_type().is_symlink() {
                    return Err(PathError::SymlinkComponent { path: traversed });
                }
                directory = Some(
                    parent
                        .open_dir_nofollow(name)
                        .map_err(|source| io_error(&traversed, source))?,
                );
            }
            Component::CurDir
            | Component::ParentDir
            | Component::Prefix(_)
            | Component::RootDir => {
                return Err(PathError::InvalidPath {
                    path: path.to_path_buf(),
                });
            }
        }
    }
    match directory {
        Some(directory) => Ok(directory),
        None => Dir::open_ambient_dir(&anchor, ambient_authority())
            .map_err(|source| io_error(path, source)),
    }
}

pub(super) fn identity(metadata: &impl MetadataExt) -> Identity {
    Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn io_error(path: &Path, source: io::Error) -> PathError {
    PathError::Io {
        path: path.to_path_buf(),
        source,
    }
}
