use super::worker::{map_worker, WORKER_COLUMNS};
use super::{PersistenceError, Store, WorkerRecord};

impl Store {
    /// # Errors
    /// Returns a persistence error when workers cannot be loaded or decoded.
    pub async fn workers(&self) -> Result<Vec<WorkerRecord>, PersistenceError> {
        let sql = format!("{WORKER_COLUMNS} ORDER BY name COLLATE NOCASE, id");
        sqlx::query(&sql)
            .fetch_all(self.database.pool())
            .await?
            .iter()
            .map(map_worker)
            .collect()
    }
}
