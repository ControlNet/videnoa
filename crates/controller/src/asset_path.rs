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
    ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]
        .iter()
        .any(|name| basename.eq_ignore_ascii_case(name))
        || is_numbered_device(basename, "COM")
        || is_numbered_device(basename, "LPT")
}

fn is_numbered_device(basename: &str, prefix: &str) -> bool {
    basename
        .get(..3)
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        && basename.get(3..).is_some_and(|number| {
            matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

#[cfg(test)]
mod tests {
    use percent_encoding::percent_decode_str;

    use super::ExactAssetPath;

    #[test]
    fn superscript_and_console_device_aliases_are_ineligible() {
        // Given: literal and percent-decoded Windows device aliases in normalized shapes.
        let mut paths = Vec::new();
        for name in [
            "COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³", "CONIN$", "CONOUT$",
        ] {
            paths.extend([
                format!("/assets/{name}"),
                format!("/assets/{}/nested", name.to_ascii_lowercase()),
                format!("/assets/{name}.txt"),
                format!("/assets/{name} "),
                format!("/assets/{name}."),
            ]);
        }
        for encoded in [
            "/assets/COM%C2%B9.txt",
            "/assets/LPT%C2%B3",
            "/assets/CONOUT%24.log",
        ] {
            let decoded = match percent_decode_str(encoded).decode_utf8() {
                Ok(path) => path.into_owned(),
                Err(error) => panic!("valid test path failed to decode: {error}"),
            };
            paths.push(decoded);
        }

        // When: each decoded path crosses the pure exact-asset eligibility boundary.
        for path in paths {
            // Then: no Windows device alias can become an exact disk or embedded key.
            assert!(
                ExactAssetPath::from_decoded_path(&path).is_none(),
                "eligible path: {path}"
            );
        }
    }

    #[test]
    fn representative_portable_assets_remain_eligible() {
        // Given: portable relative asset names, including Unicode and nested components.
        let paths = [
            "/assets/index.js",
            "/assets/日本語.css",
            "/fonts/Inter-Regular.woff2",
        ];

        // When/Then: each path remains an exact asset candidate.
        for path in paths {
            assert!(
                ExactAssetPath::from_decoded_path(path).is_some(),
                "ineligible path: {path}"
            );
        }
    }
}
