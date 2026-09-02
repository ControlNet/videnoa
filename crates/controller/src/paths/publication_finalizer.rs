use std::io;
use std::path::{Path, PathBuf};

use cap_std::fs::Dir;

use super::PathError;

pub(crate) struct PublicationFinalizer {
    directory: Dir,
    staging: PathBuf,
    destination: PathBuf,
    display_parent: PathBuf,
}

impl PublicationFinalizer {
    pub(super) fn new(
        directory: Dir,
        staging: PathBuf,
        destination: PathBuf,
        display_parent: PathBuf,
    ) -> Self {
        Self {
            directory,
            staging,
            destination,
            display_parent,
        }
    }

    pub(crate) fn rename_noreplace(&self) -> Result<(), PathError> {
        rename_relative(
            &self.directory,
            &self.staging,
            &self.destination,
            &self.display_parent,
        )
    }

    pub(crate) fn sync_parent(&self) -> Result<(), PathError> {
        sync_directory(&self.directory).map_err(|source| io_error(&self.display_parent, source))
    }
}

#[cfg(target_os = "linux")]
fn rename_relative(
    directory: &Dir,
    staging: &Path,
    destination: &Path,
    display_parent: &Path,
) -> Result<(), PathError> {
    rustix::fs::renameat_with(
        directory,
        staging,
        directory,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|source| io_error(display_parent, std::io::Error::from(source)))
}

#[cfg(windows)]
fn rename_relative(
    _directory: &Dir,
    staging: &Path,
    destination: &Path,
    display_parent: &Path,
) -> Result<(), PathError> {
    renamore::rename_exclusive(
        display_parent.join(staging),
        display_parent.join(destination),
    )
    .map_err(|source| io_error(display_parent, source))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn rename_relative(
    _directory: &Dir,
    _staging: &Path,
    _destination: &Path,
    display_parent: &Path,
) -> Result<(), PathError> {
    Err(io_error(
        display_parent,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "atomic no-replace publication is unsupported on this platform",
        ),
    ))
}

#[cfg(unix)]
fn sync_directory(directory: &Dir) -> Result<(), io::Error> {
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
fn sync_directory(directory: &Dir) -> Result<(), io::Error> {
    directory.try_clone()?.into_std_file().sync_all()
}

#[cfg(not(any(unix, windows)))]
fn sync_directory(_directory: &Dir) -> Result<(), io::Error> {
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
