use std::io;
use std::path::{Path, PathBuf};

use cap_std::fs::Dir;

use super::PathError;

pub(crate) struct PublicationFinalizer {
    source_directory: Dir,
    source: PathBuf,
    display_source_parent: PathBuf,
    destination_directory: Dir,
    destination: PathBuf,
    display_destination_parent: PathBuf,
}

impl PublicationFinalizer {
    pub(super) fn new(
        source_directory: Dir,
        source: PathBuf,
        display_source_parent: PathBuf,
        destination_directory: Dir,
        destination: PathBuf,
        display_destination_parent: PathBuf,
    ) -> Self {
        Self {
            source_directory,
            source,
            display_source_parent,
            destination_directory,
            destination,
            display_destination_parent,
        }
    }

    pub(crate) fn rename_noreplace(&self) -> Result<(), PathError> {
        rename_relative(
            &self.source_directory,
            &self.source,
            &self.display_source_parent,
            &self.destination_directory,
            &self.destination,
            &self.display_destination_parent,
        )
    }

    pub(crate) fn sync_parents(&self) -> Result<(), PathError> {
        sync_directory(&self.destination_directory)
            .map_err(|source| io_error(&self.display_destination_parent, source))?;
        sync_directory(&self.source_directory)
            .map_err(|source| io_error(&self.display_source_parent, source))
    }
}

#[cfg(target_os = "linux")]
fn rename_relative(
    source_directory: &Dir,
    source: &Path,
    display_source_parent: &Path,
    destination_directory: &Dir,
    destination: &Path,
    display_destination_parent: &Path,
) -> Result<(), PathError> {
    rustix::fs::renameat_with(
        source_directory,
        source,
        destination_directory,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|error| {
        if error == rustix::io::Errno::XDEV {
            PathError::CrossFilesystemPublication {
                source_path: display_source_parent.join(source),
                destination: display_destination_parent.join(destination),
            }
        } else {
            io_error(display_destination_parent, std::io::Error::from(error))
        }
    })
}

#[cfg(windows)]
fn rename_relative(
    _source_directory: &Dir,
    source: &Path,
    display_source_parent: &Path,
    _destination_directory: &Dir,
    destination: &Path,
    display_destination_parent: &Path,
) -> Result<(), PathError> {
    const ERROR_NOT_SAME_DEVICE: i32 = 17;
    renamore::rename_exclusive(
        display_source_parent.join(source),
        display_destination_parent.join(destination),
    )
    .map_err(|error| {
        if error.raw_os_error() == Some(ERROR_NOT_SAME_DEVICE) {
            PathError::CrossFilesystemPublication {
                source_path: display_source_parent.join(source),
                destination: display_destination_parent.join(destination),
            }
        } else {
            io_error(display_destination_parent, error)
        }
    })
}

#[cfg(not(any(target_os = "linux", windows)))]
fn rename_relative(
    _source_directory: &Dir,
    _source: &Path,
    _display_source_parent: &Path,
    _destination_directory: &Dir,
    _destination: &Path,
    display_destination_parent: &Path,
) -> Result<(), PathError> {
    Err(io_error(
        display_destination_parent,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "atomic no-replace publication is unsupported on this platform",
        ),
    ))
}

#[cfg(unix)]
pub(super) fn sync_directory(directory: &Dir) -> Result<(), io::Error> {
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
pub(super) fn sync_directory(directory: &Dir) -> Result<(), io::Error> {
    directory.try_clone()?.into_std_file().sync_all()
}

#[cfg(not(any(unix, windows)))]
pub(super) fn sync_directory(_directory: &Dir) -> Result<(), io::Error> {
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
