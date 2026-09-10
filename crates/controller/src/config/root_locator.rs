use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use serde::{Deserialize, Serialize};

use super::{private, ConfigError};

const LOCATOR_FILE: &str = ".videnoa-data-root.toml";
const LOCATOR_PENDING: &str = ".videnoa-data-root.toml.pending";
const MIGRATION_MARKER: &str = ".videnoa-data-root-migration.toml";
const MIGRATION_MARKER_PENDING: &str = ".videnoa-data-root-migration.toml.pending";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RootLocator {
    data_root: PathBuf,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum MigrationState {
    Copying,
    Active,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MigrationMarker {
    source: PathBuf,
    state: MigrationState,
}

pub(super) fn load(bootstrap_root: &Path) -> Result<Option<PathBuf>, ConfigError> {
    let path = bootstrap_root.join(LOCATOR_FILE);
    reject_symlink(&path, "data_root_locator")?;
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(path, error)),
    };
    let locator: RootLocator = toml::from_str(&source).map_err(|error| ConfigError::Schema {
        detail: format!("data root locator is invalid: {error}"),
    })?;
    validate_absolute_root("data_root_locator", &locator.data_root)?;
    Ok(Some(locator.data_root.components().collect()))
}

pub(super) fn persist(bootstrap_root: &Path, data_root: &Path) -> Result<(), ConfigError> {
    let source = toml::to_string(&RootLocator {
        data_root: data_root.to_path_buf(),
    })
    .map_err(|error| ConfigError::Schema {
        detail: error.to_string(),
    })?;
    replace_private_file(
        &bootstrap_root.join(LOCATOR_FILE),
        &bootstrap_root.join(LOCATOR_PENDING),
        &source,
        "data_root_locator",
    )
}

pub(super) fn migrate_data_root(source: &Path, destination: &Path) -> Result<(), ConfigError> {
    if source == destination {
        return Ok(());
    }
    if source.starts_with(destination) || destination.starts_with(source) {
        return Err(ConfigError::InvalidRoot {
            field: "paths.data_root",
            path: destination.to_path_buf(),
            reason: "current and new data roots must not contain each other",
        });
    }
    let destination = private::prepare_root(destination, "paths.data_root")?;
    let marker = destination.join(MIGRATION_MARKER);
    if prepare_destination(&destination, &marker, source)? == MigrationState::Active {
        return Ok(());
    }
    copy_directory(source, &destination)?;
    persist_migration_marker(&destination, source, MigrationState::Active)
}

pub(super) fn retained_sources(active_root: &Path) -> Result<Vec<PathBuf>, ConfigError> {
    let mut roots = Vec::new();
    let mut current = active_root.to_path_buf();
    let mut visited = std::collections::HashSet::from([current.clone()]);
    loop {
        let marker = current.join(MIGRATION_MARKER);
        reject_symlink(&marker, "data_root_migration_marker")?;
        let source = match fs::read_to_string(&marker) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(io_error(marker, error)),
        };
        let marker: MigrationMarker =
            toml::from_str(&source).map_err(|_| ConfigError::InvalidRoot {
                field: "paths.data_root",
                path: current.clone(),
                reason: "destination migration marker is invalid",
            })?;
        validate_absolute_root("paths.data_root", &marker.source)?;
        if !visited.insert(marker.source.clone()) {
            return Err(ConfigError::InvalidRoot {
                field: "paths.data_root",
                path: marker.source,
                reason: "data root migration history contains a cycle",
            });
        }
        if !marker.source.is_dir() {
            break;
        }
        current.clone_from(&marker.source);
        roots.push(marker.source);
    }
    Ok(roots)
}

fn prepare_destination(
    destination: &Path,
    marker: &Path,
    source: &Path,
) -> Result<MigrationState, ConfigError> {
    let mut entries =
        fs::read_dir(destination).map_err(|error| io_error(destination.to_path_buf(), error))?;
    if entries
        .next()
        .transpose()
        .map_err(|error| io_error(destination.to_path_buf(), error))?
        .is_none()
    {
        persist_migration_marker(destination, source, MigrationState::Copying)?;
        return Ok(MigrationState::Copying);
    }
    let recorded = fs::read_to_string(marker).map_err(|error| ConfigError::InvalidRoot {
        field: "paths.data_root",
        path: destination.to_path_buf(),
        reason: if error.kind() == std::io::ErrorKind::NotFound {
            "destination must be empty"
        } else {
            "destination migration marker is unreadable"
        },
    })?;
    let recorded: MigrationMarker =
        toml::from_str(&recorded).map_err(|_| ConfigError::InvalidRoot {
            field: "paths.data_root",
            path: destination.to_path_buf(),
            reason: "destination migration marker is invalid",
        })?;
    if recorded.source != source {
        return Err(ConfigError::InvalidRoot {
            field: "paths.data_root",
            path: destination.to_path_buf(),
            reason: "destination belongs to a different migration",
        });
    }
    Ok(recorded.state)
}

fn persist_migration_marker(
    destination: &Path,
    source: &Path,
    state: MigrationState,
) -> Result<(), ConfigError> {
    let contents = toml::to_string(&MigrationMarker {
        source: source.to_path_buf(),
        state,
    })
    .map_err(|error| ConfigError::Schema {
        detail: error.to_string(),
    })?;
    replace_private_file(
        &destination.join(MIGRATION_MARKER),
        &destination.join(MIGRATION_MARKER_PENDING),
        &contents,
        "data_root_migration_marker",
    )
}

fn replace_private_file(
    path: &Path,
    pending: &Path,
    contents: &str,
    field: &'static str,
) -> Result<(), ConfigError> {
    reject_symlink(path, field)?;
    reject_symlink(pending, field)?;
    match fs::remove_file(pending) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(pending.to_path_buf(), error)),
    }
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(pending)
        .map_err(|error| io_error(pending.to_path_buf(), error))?;
    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| io_error(pending.to_path_buf(), error))?;
    fs::rename(pending, path).map_err(|error| io_error(path.to_path_buf(), error))?;
    let parent = path.parent().ok_or_else(|| {
        io_error(
            path.to_path_buf(),
            std::io::Error::other("private file has no parent"),
        )
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error(parent.to_path_buf(), error))
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), ConfigError> {
    for entry in fs::read_dir(source).map_err(|error| io_error(source.to_path_buf(), error))? {
        let entry = entry.map_err(|error| io_error(source.to_path_buf(), error))?;
        if entry.file_name() == MIGRATION_MARKER
            || entry.file_name() == MIGRATION_MARKER_PENDING
            || entry.file_name() == LOCATOR_FILE
            || entry.file_name() == LOCATOR_PENDING
        {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| io_error(source_path.clone(), error))?;
        if metadata.file_type().is_symlink() {
            return Err(ConfigError::InvalidRoot {
                field: "paths.data_root",
                path: source_path,
                reason: "symbolic links are not allowed in Controller data",
            });
        }
        if metadata.is_dir() {
            match fs::create_dir(&destination_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if !destination_path.is_dir() {
                        return Err(io_error(destination_path, error));
                    }
                }
                Err(error) => return Err(io_error(destination_path, error)),
            }
            #[cfg(unix)]
            fs::set_permissions(
                &destination_path,
                fs::Permissions::from_mode(metadata.permissions().mode()),
            )
            .map_err(|error| io_error(destination_path.clone(), error))?;
            copy_directory(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|error| io_error(destination_path.clone(), error))?;
            File::open(&destination_path)
                .and_then(|file| file.sync_all())
                .map_err(|error| io_error(destination_path, error))?;
        } else {
            return Err(ConfigError::InvalidRoot {
                field: "paths.data_root",
                path: source_path,
                reason: "Controller data contains a non-regular entry",
            });
        }
    }
    File::open(destination)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error(destination.to_path_buf(), error))
}

fn validate_absolute_root(field: &'static str, path: &Path) -> Result<(), ConfigError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(ConfigError::InvalidRoot {
            field,
            path: path.to_path_buf(),
            reason: "path must be absolute and contain no parent traversal",
        });
    }
    Ok(())
}

fn reject_symlink(path: &Path, field: &'static str) -> Result<(), ConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(ConfigError::InvalidRoot {
            field,
            path: path.to_path_buf(),
            reason: "symbolic links are not allowed",
        }),
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(ConfigError::InvalidRoot {
            field,
            path: path.to_path_buf(),
            reason: "path is not a regular file",
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(path.to_path_buf(), error)),
    }
}

fn io_error(path: PathBuf, source: std::io::Error) -> ConfigError {
    ConfigError::Io { path, source }
}
