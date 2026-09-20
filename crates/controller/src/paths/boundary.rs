use std::path::{Component, Path, PathBuf};

use super::{PathCapabilities, PathError, Root};

impl PathCapabilities {
    pub(super) fn media_spelling(&self, path: &Path, roots: &[Root]) -> Result<PathBuf, PathError> {
        self.data.ensure_current()?;
        self.temp.ensure_current()?;
        self.additional_private
            .iter()
            .try_for_each(Root::ensure_current)?;
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
        super::root::validate_absolute(&path)?;
        if path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(PathError::InvalidPath { path });
        }
        let path: PathBuf = path.components().collect();
        if self.is_private(&path) {
            return Err(PathError::OutsideRoots { path });
        }
        Ok(path)
    }

    pub(super) fn media_output_path(&self, path: &Path) -> Result<PathBuf, PathError> {
        let path = self.media_spelling(path, &self.outputs)?;
        let leaf = path
            .file_name()
            .ok_or_else(|| PathError::InvalidPath { path: path.clone() })?;
        let parent = path
            .parent()
            .ok_or_else(|| PathError::InvalidPath { path: path.clone() })?;
        // A final output link is an existing destination, never a new write target
        // or proof that this task published the linked file.
        Ok(self.media_path(parent, &self.outputs)?.join(leaf))
    }

    pub(super) fn media_path(&self, path: &Path, roots: &[Root]) -> Result<PathBuf, PathError> {
        let path = self.media_spelling(path, roots)?;
        let resolved = resolve_media_path(&path)?;
        if self.is_private(&resolved) {
            return Err(PathError::OutsideRoots { path });
        }
        Ok(resolved)
    }

    fn is_private(&self, path: &Path) -> bool {
        reserved(path, self.data.display_path())
            || reserved(path, self.temp.display_path())
            || self
                .additional_private
                .iter()
                .any(|root| reserved(path, root.display_path()))
    }
}

// Resolve the existing prefix, retaining missing output components or glob segments.
// Actual I/O still opens the resolved path with descriptor-backed no-follow checks.
pub(super) fn resolve_media_path(path: &Path) -> Result<PathBuf, PathError> {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let mut resolved = std::fs::canonicalize(ancestor)
                    .map_err(|source| super::io_error(ancestor, source))?;
                for component in suffix.iter().rev() {
                    resolved.push(component);
                }
                return Ok(normalize_resolved(resolved));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let leaf = ancestor.file_name().ok_or_else(|| PathError::InvalidPath {
                    path: path.to_owned(),
                })?;
                suffix.push(leaf);
                ancestor = ancestor.parent().ok_or_else(|| PathError::InvalidPath {
                    path: path.to_owned(),
                })?;
            }
            Err(source) => return Err(super::io_error(ancestor, source)),
        }
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

#[cfg(not(windows))]
fn normalize_resolved(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
fn normalize_resolved(path: PathBuf) -> PathBuf {
    use std::path::Prefix;
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path;
    };
    let mut result = match prefix.kind() {
        Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:", char::from(drive))),
        Prefix::VerbatimUNC(server, share) => PathBuf::from(r"\\").join(server).join(share),
        _ => return path,
    };
    for component in components {
        result.push(component.as_os_str());
    }
    result
}
