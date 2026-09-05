use std::path::{Component, Path, PathBuf};

use super::{PathCapabilities, PathError, Root};

impl PathCapabilities {
    pub(super) fn media_path(&self, path: &Path, roots: &[Root]) -> Result<PathBuf, PathError> {
        self.data.ensure_current()?;
        self.temp.ensure_current()?;
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            let [root] = roots else {
                return Err(PathError::InvalidPath {
                    path: path.to_path_buf(),
                });
            };
            if path.as_os_str().is_empty()
                || path
                    .components()
                    .any(|component| !matches!(component, Component::Normal(_)))
            {
                return Err(PathError::InvalidPath {
                    path: path.to_path_buf(),
                });
            }
            root.display_path().join(path)
        };
        let path: PathBuf = path.components().collect();
        if reserved(&path, self.data.display_path()) || reserved(&path, self.temp.display_path()) {
            return Err(PathError::OutsideRoots { path });
        }
        Ok(path)
    }
}

fn reserved(path: &Path, boundary: &Path) -> bool {
    #[cfg(not(windows))]
    {
        path.starts_with(boundary)
    }
    #[cfg(windows)]
    {
        let path = windows_spelling(path);
        let boundary = windows_spelling(boundary);
        path == boundary
            || path
                .strip_prefix(&boundary)
                .is_some_and(|suffix| suffix.starts_with('/'))
    }
}

#[cfg(windows)]
fn windows_spelling(path: &Path) -> String {
    let spelling = path.to_string_lossy().replace('\\', "/").to_lowercase();
    if let Some(unc) = spelling.strip_prefix("//?/unc/") {
        format!("//{unc}")
    } else {
        spelling
            .strip_prefix("//?/")
            .unwrap_or(&spelling)
            .to_owned()
    }
}
