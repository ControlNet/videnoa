use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

use super::ConfigError;

pub(super) fn prepare_data_root(workspace: &Path) -> Result<PathBuf, ConfigError> {
    prepare_root(&workspace.join("data"), "data_root")
}

pub(super) fn prepare_root(path: &Path, field: &'static str) -> Result<PathBuf, ConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(redirected(field, path));
        }
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(ConfigError::InvalidRoot {
                field,
                path: path.to_path_buf(),
                reason: "path is not a directory",
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            builder.mode(0o700);
            builder.create(path).map_err(|source| ConfigError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        }
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    }
    make_private_directory(path)?;
    let canonical = fs::canonicalize(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if canonical != path {
        return Err(redirected(field, path));
    }
    Ok(canonical)
}

pub(super) fn prepare_config_file(path: &Path) -> Result<(), ConfigError> {
    reject_symlink(path, "config_file")?;
    #[cfg(unix)]
    match fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    }
    Ok(())
}

fn make_private_directory(path: &Path) -> Result<(), ConfigError> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
        ConfigError::Io {
            path: path.to_path_buf(),
            source,
        }
    })?;
    Ok(())
}

fn reject_symlink(path: &Path, field: &'static str) -> Result<(), ConfigError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(redirected(field, path)),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(ConfigError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn redirected(field: &'static str, path: &Path) -> ConfigError {
    ConfigError::InvalidRoot {
        field,
        path: path.to_path_buf(),
        reason: "symbolic links are not allowed",
    }
}
