use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns a persistence error when the migrated settings row cannot be read.
    pub async fn check_ready(&self) -> Result<(), PersistenceError> {
        self.settings().await.map(|_| ())
    }
}
