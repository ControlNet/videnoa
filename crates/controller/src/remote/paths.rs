use crate::domain::RemotePath;

use super::VidenoaClientError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileApiPath(String);

impl FileApiPath {
    /// Parses a workspace-relative file API path without local path normalization.
    ///
    /// # Errors
    /// Returns [`VidenoaClientError::InvalidFilePath`] for empty or unsafe path syntax.
    pub fn parse(value: &str) -> Result<Self, VidenoaClientError> {
        let valid = !value.is_empty()
            && !value.starts_with(['/', '\\'])
            && !value.contains('\\')
            && value
                .split('/')
                .all(|segment| !segment.is_empty() && !matches!(segment, "." | ".."));
        if !valid {
            return Err(VidenoaClientError::InvalidFilePath);
        }
        Ok(Self(value.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(super) fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

/// Derives a sibling workflow path while preserving remote spelling, including `..` segments.
///
/// # Errors
/// Returns [`VidenoaClientError::InvalidFilePath`] when the sibling name is not one path leaf.
pub fn sibling_output_path(
    uploaded_path: &RemotePath,
    sibling_name: &str,
) -> Result<RemotePath, VidenoaClientError> {
    if sibling_name.is_empty() || sibling_name.contains(['/', '\\']) {
        return Err(VidenoaClientError::InvalidFilePath);
    }
    let value = match uploaded_path.as_str().rsplit_once('/') {
        Some((parent, _)) => format!("{parent}/{sibling_name}"),
        None => sibling_name.to_owned(),
    };
    Ok(RemotePath::new(value))
}
