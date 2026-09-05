use std::path::Path;

use cap_std::fs::File;

use super::{content_identity, identity, io_error, InputSnapshot, PathError, RootedInput};

impl RootedInput {
    #[must_use]
    pub const fn snapshot(&self) -> &InputSnapshot {
        &self.snapshot
    }

    #[must_use]
    pub fn display_path(&self) -> &Path {
        &self.display_path
    }

    /// Reopens the input and requires metadata and content identity to match.
    ///
    /// # Errors
    /// Returns [`PathError::InputChanged`] when the accepted input was replaced or modified.
    pub fn reopen_checked(&self) -> Result<File, PathError> {
        self.root.ensure_current()?;
        let mut file = self.root.open_file(&self.relative, true)?;
        let metadata = file
            .metadata()
            .map_err(|source| io_error(&self.display_path, source))?;
        let modified = metadata
            .modified()
            .map_err(|source| io_error(&self.display_path, source))?
            .into_std();
        if !metadata.is_file()
            || identity(&metadata) != self.snapshot.identity
            || metadata.len() != self.snapshot.length
            || modified != self.snapshot.modified
        {
            return Err(PathError::InputChanged {
                path: self.display_path.clone(),
            });
        }
        let current_content_identity = content_identity(&mut file, &self.display_path)?;
        let current = file
            .metadata()
            .map_err(|source| io_error(&self.display_path, source))?;
        let current_modified = current
            .modified()
            .map_err(|source| io_error(&self.display_path, source))?
            .into_std();
        if identity(&current) != self.snapshot.identity
            || current.len() != self.snapshot.length
            || current_modified != self.snapshot.modified
            || current_content_identity != self.snapshot.content_identity
        {
            return Err(PathError::InputChanged {
                path: self.display_path.clone(),
            });
        }
        Ok(file)
    }
}
