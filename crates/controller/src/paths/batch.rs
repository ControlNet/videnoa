use std::path::{Component, Path, PathBuf};

use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
use glob::{MatchOptions, Pattern};

use super::{PathCapabilities, PathError, Root};

const MAX_ENTRIES: usize = 20_000;
const MAX_MATCHES: usize = 500;
const MAX_DEPTH: usize = 64;

impl PathCapabilities {
    // Preview checks metadata only. Full content identity is captured at task intake.
    pub(crate) fn check_preview_input(&self, path: &Path) -> Result<(), PathError> {
        let path = self.media_path(path, &self.inputs)?;
        let (root, relative) = super::root::select_root(&self.inputs, &path)?;
        let file = root.open_file(&relative, false)?;
        let metadata = file
            .metadata()
            .map_err(|source| super::io_error(&path, source))?;
        if !metadata.is_file() {
            return Err(PathError::InputNotRegular { path });
        }
        root.ensure_current()
    }

    pub(crate) fn match_inputs(&self, pattern: &str) -> Result<Vec<PathBuf>, &'static str> {
        if pattern.is_empty() || pattern.len() > 4096 || pattern.contains('\0') {
            return Err("Enter an input pattern of 1 to 4096 bytes.");
        }
        let absolute = self
            .media_path(Path::new(pattern), &self.inputs)
            .map_err(|_| "Input pattern is unsafe or points into private Controller storage.")?;
        let mut base = PathBuf::new();
        let mut segments = Vec::new();
        for component in absolute.components() {
            match component {
                Component::Prefix(_) | Component::RootDir if segments.is_empty() => {
                    base.push(component);
                }
                Component::Normal(name) => {
                    let name = name.to_str().ok_or("Input pattern must be UTF-8.")?;
                    if segments.is_empty() && !name.contains(['*', '?', '[', '{', '}']) {
                        base.push(name);
                    } else {
                        segments.push(name.to_owned());
                    }
                }
                _ => return Err("Parent traversal and ambiguous paths are not allowed."),
            }
        }
        if segments.is_empty() {
            self.check_preview_input(&base)
                .map_err(|_| "Input must be a safe, accessible regular file.")?;
            return Ok(vec![base]);
        }
        self.media_path(&base, &self.inputs)
            .map_err(|_| "Input directory is private.")?;
        let patterns = segments
            .iter()
            .map(|segment| super::batch_pattern::compile(segment))
            .collect::<Result<Vec<_>, _>>()?;
        let root = Root::open(&base).map_err(|_| "Input directory is unsafe or unavailable.")?;
        let directory = root
            .clone_directory()
            .map_err(|_| "Input directory is unavailable.")?;
        let mut scan = Scan {
            paths: self,
            entries: 0,
            matches: std::collections::BTreeSet::new(),
        };
        scan.walk(&directory, &base, &segments, &patterns, 0, 0)?;
        root.ensure_current()
            .map_err(|_| "Input directory changed while scanning.")?;
        self.check_ready()
            .map_err(|_| "Controller paths changed while scanning.")?;
        Ok(scan.matches.into_iter().collect())
    }
}

struct Scan<'a> {
    paths: &'a PathCapabilities,
    entries: usize,
    matches: std::collections::BTreeSet<PathBuf>,
}

impl Scan<'_> {
    fn walk(
        &mut self,
        directory: &Dir,
        path: &Path,
        segments: &[String],
        patterns: &[Vec<Pattern>],
        index: usize,
        depth: usize,
    ) -> Result<(), &'static str> {
        if depth > MAX_DEPTH {
            return Err("Pattern exceeds 64 directory levels. Narrow the input pattern.");
        }
        let recursive = segments[index] == "**";
        if recursive && index + 1 < segments.len() {
            self.walk(directory, path, segments, patterns, index + 1, depth)?;
        }
        for entry in directory
            .entries()
            .map_err(|_| "A matched directory cannot be read.")?
        {
            self.entries += 1;
            if self.entries > MAX_ENTRIES {
                return Err("Pattern exceeds 20000 scanned entries. Narrow the input pattern.");
            }
            let entry =
                entry.map_err(|_| "A directory changed or became unreadable during preview.")?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let matches = recursive
                || patterns[index].iter().any(|pattern| {
                    pattern.matches_with(
                        name,
                        MatchOptions {
                            case_sensitive: !cfg!(windows),
                            require_literal_separator: true,
                            require_literal_leading_dot: true,
                        },
                    )
                });
            if !matches {
                continue;
            }
            let child = path.join(name);
            if self.paths.media_path(&child, &self.paths.inputs).is_err() {
                continue;
            }
            let metadata = directory
                .symlink_metadata(name)
                .map_err(|_| "A matched path changed while scanning.")?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_file() && index + 1 == segments.len() {
                self.matches.insert(child);
                if self.matches.len() > MAX_MATCHES {
                    return Err("Pattern matches more than 500 files. Narrow the input pattern.");
                }
            } else if metadata.is_dir() && (recursive || index + 1 < segments.len()) {
                let nested = directory
                    .open_dir_nofollow(name)
                    .map_err(|_| "A matched directory is unsafe or unavailable.")?;
                self.walk(
                    &nested,
                    &child,
                    segments,
                    patterns,
                    if recursive { index } else { index + 1 },
                    depth + 1,
                )?;
            }
        }
        Ok(())
    }
}
