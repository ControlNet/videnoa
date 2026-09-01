#[cfg(debug_assertions)]
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug)]
pub(crate) struct ExactAssetPath<'a>(&'a str);

impl<'a> ExactAssetPath<'a> {
    pub(crate) fn from_decoded_path(decoded_path: &'a str) -> Option<Self> {
        let requested_path = decoded_path.trim_start_matches('/');
        let asset_path = if requested_path.is_empty() {
            "index.html"
        } else if requested_path.ends_with('/') {
            return None;
        } else {
            requested_path
        };

        asset_path
            .split('/')
            .all(is_safe_component)
            .then_some(Self(asset_path))
    }

    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }

    #[cfg(debug_assertions)]
    pub(crate) fn join_to(self, root: &Path) -> PathBuf {
        self.0
            .split('/')
            .fold(root.to_path_buf(), |path, component| path.join(component))
    }
}

fn is_safe_component(component: &str) -> bool {
    !component.is_empty()
        && !component.ends_with(' ')
        && !component.ends_with('.')
        && !component
            .chars()
            .any(|character| matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        && !is_reserved_device(component)
}

fn is_reserved_device(component: &str) -> bool {
    let basename = component
        .split_once('.')
        .map_or(component, |(basename, _)| basename)
        .trim_end_matches([' ', '.']);
    ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|name| basename.eq_ignore_ascii_case(name))
        || is_numbered_device(basename, "COM")
        || is_numbered_device(basename, "LPT")
}

fn is_numbered_device(basename: &str, prefix: &str) -> bool {
    basename.len() == 4
        && basename
            .get(..3)
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        && basename
            .as_bytes()
            .get(3)
            .is_some_and(|number| (b'1'..=b'9').contains(number))
}
