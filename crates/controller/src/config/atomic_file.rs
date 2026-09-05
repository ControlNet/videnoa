use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use super::ConfigError;

pub(super) fn replace(path: &Path, contents: &str) -> Result<(), ConfigError> {
    let parent = path.parent().ok_or_else(|| ConfigError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::other("configuration path has no parent"),
    })?;
    reject_symlink(parent, "data_root")?;
    reject_symlink(path, "config_file")?;
    let temporary = parent.join(".controller.toml.pending");
    match fs::symlink_metadata(&temporary) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(redirected("config_temporary", &temporary));
        }
        Ok(metadata) if metadata.is_file() => {
            fs::remove_file(&temporary).map_err(|source| ConfigError::Io {
                path: temporary.clone(),
                source,
            })?;
        }
        Ok(_) => {
            return Err(ConfigError::InvalidRoot {
                field: "config_temporary",
                path: temporary,
                reason: "path is not a regular file",
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(ConfigError::Io {
                path: temporary,
                source,
            });
        }
    }
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temporary).map_err(|source| ConfigError::Io {
        path: temporary.clone(),
        source,
    })?;
    file.write_all(contents.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|source| ConfigError::Io {
            path: temporary.clone(),
            source,
        })?;
    fs::rename(&temporary, path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| ConfigError::Io {
            path: parent.to_path_buf(),
            source,
        })
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
