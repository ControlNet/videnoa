use super::{PersistenceError, Store};

impl Store {
    /// # Errors
    /// Returns a persistence error when the database cannot be queried.
    pub async fn check_ready(&self) -> Result<(), PersistenceError> {
        sqlx::query("SELECT 1")
            .execute(self.database.pool())
            .await?;
        Ok(())
    }
}
