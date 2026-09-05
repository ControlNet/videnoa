use std::path::{Component, Path, PathBuf};

use super::{PathCapabilities, PathError, Root};

impl PathCapabilities {
    pub(super) fn media_path(&self, path: &Path, roots: &[Root]) -> Result<PathBuf, PathError> {
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
        let mut path_components = path.components();
        boundary.components().all(|component| {
            path_components.next().is_some_and(|candidate| {
                candidate
                    .as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&component.as_os_str().to_string_lossy())
            })
        })
    }
}
