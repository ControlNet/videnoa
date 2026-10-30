//! Where the desktop app keeps its state.
//!
//! Earlier builds used `./data`, which for a desktop launch is wherever the
//! shortcut or file manager started the process (often the install folder,
//! which may be read-only). The desktop app now uses the platform's local
//! app-data directory (`%LOCALAPPDATA%\com.videnoa.app`,
//! `~/.local/share/com.videnoa.app`, `~/Library/Application Support/com.videnoa.app`)
//! and moves an existing `<exe_dir>/data` there once. `VIDENOA_DATA_DIR`
//! still overrides everything.

use std::path::{Path, PathBuf};

/// The Tauri bundle identifier, used as the app-data folder name.
const APP_IDENTIFIER: &str = "com.videnoa.app";
const ENV_DATA_DIR: &str = "VIDENOA_DATA_DIR";

#[derive(Debug, PartialEq, Eq)]
enum DataDirPlan {
    Use(PathBuf),
    /// Move the data directory of an earlier build to `target`.
    Move {
        legacy: PathBuf,
        target: PathBuf,
    },
}

/// The chosen data directory and, when it involved moving or keeping an old
/// one, a message to log once logging is up.
pub struct DesktopDataDir {
    pub path: PathBuf,
    pub note: Option<String>,
}

pub fn resolve() -> DesktopDataDir {
    let env_override = std::env::var_os(ENV_DATA_DIR).map(PathBuf::from);
    let legacy = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("data")));
    apply(plan(env_override, dirs::data_local_dir(), legacy))
}

fn plan(
    env_override: Option<PathBuf>,
    app_data_root: Option<PathBuf>,
    legacy: Option<PathBuf>,
) -> DataDirPlan {
    if let Some(dir) = env_override {
        return DataDirPlan::Use(dir);
    }
    let Some(target) = app_data_root.map(|root| root.join(APP_IDENTIFIER)) else {
        return DataDirPlan::Use(legacy.unwrap_or_else(|| PathBuf::from("data")));
    };
    match legacy {
        Some(legacy) if is_data_dir(&legacy) && !target.exists() => {
            DataDirPlan::Move { legacy, target }
        }
        _ => DataDirPlan::Use(target),
    }
}

/// A directory an earlier build initialized (it always writes config.toml).
fn is_data_dir(dir: &Path) -> bool {
    videnoa_core::config::config_path(dir).is_file()
}

fn apply(plan: DataDirPlan) -> DesktopDataDir {
    match plan {
        DataDirPlan::Use(path) => DesktopDataDir { path, note: None },
        DataDirPlan::Move { legacy, target } => {
            let moved = target
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::rename(&legacy, &target));
            match moved {
                Ok(()) => DesktopDataDir {
                    note: Some(format!(
                        "Moved the data directory from {} to {}",
                        legacy.display(),
                        target.display()
                    )),
                    path: target,
                },
                // A rename fails across file systems or without write access
                // to the old folder; keep using it rather than start empty.
                Err(error) => DesktopDataDir {
                    note: Some(format!(
                        "Could not move the data directory from {} to {} ({error}); still using the old location. Move it by hand or set {ENV_DATA_DIR}.",
                        legacy.display(),
                        target.display()
                    )),
                    path: legacy,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "videnoa-desktop-data-{}-{nanos}",
            std::process::id()
        ))
    }

    #[test]
    fn the_environment_override_wins() {
        assert_eq!(
            plan(
                Some(PathBuf::from("/custom")),
                Some(PathBuf::from("/appdata")),
                Some(PathBuf::from("/opt/videnoa/data")),
            ),
            DataDirPlan::Use(PathBuf::from("/custom"))
        );
    }

    #[test]
    fn a_fresh_install_uses_the_app_data_dir() {
        let root = unique_temp_dir();
        assert_eq!(
            plan(
                None,
                Some(root.join("appdata")),
                Some(root.join("bundle/data"))
            ),
            DataDirPlan::Use(root.join("appdata").join(APP_IDENTIFIER))
        );
    }

    #[test]
    fn data_beside_the_executable_is_moved_into_the_app_data_dir_once() {
        // Given: an earlier build's data directory beside the executable.
        let root = unique_temp_dir();
        let legacy = root.join("bundle/data");
        std::fs::create_dir_all(legacy.join("workflows")).unwrap();
        std::fs::write(legacy.join("config.toml"), "").unwrap();
        std::fs::write(legacy.join("workflows/mine.json"), "{}").unwrap();
        let app_data = root.join("appdata");

        // When: the desktop app starts.
        let chosen = apply(plan(None, Some(app_data.clone()), Some(legacy.clone())));

        // Then: the data now lives in the app-data directory.
        let target = app_data.join(APP_IDENTIFIER);
        assert_eq!(chosen.path, target);
        assert!(chosen.note.is_some());
        assert!(target.join("workflows/mine.json").is_file());
        assert!(!legacy.exists());

        // A later start just uses it, even if an old folder reappears.
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("config.toml"), "").unwrap();
        assert_eq!(
            plan(None, Some(app_data), Some(legacy)),
            DataDirPlan::Use(target)
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_failed_move_keeps_using_the_old_data_dir() {
        let root = unique_temp_dir();
        let legacy = root.join("bundle/data");
        std::fs::create_dir_all(&legacy).unwrap();
        // The target's parent is a file, so it cannot be created.
        std::fs::write(root.join("appdata"), "").unwrap();

        let chosen = apply(DataDirPlan::Move {
            legacy: legacy.clone(),
            target: root.join("appdata").join(APP_IDENTIFIER),
        });

        assert_eq!(chosen.path, legacy);
        assert!(chosen
            .note
            .unwrap()
            .contains("still using the old location"));
        std::fs::remove_dir_all(&root).ok();
    }
}
