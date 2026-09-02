#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("database operation failed")]
    Database(#[from] sqlx::Error),
    #[error("database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("persisted field `{field}` contains corrupt value `{value}`")]
    CorruptValue { field: &'static str, value: String },
    #[error("JSON field `{field}` could not be encoded or decoded")]
    Json {
        field: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("numeric field `{field}` cannot be represented by SQLite")]
    NumericOverflow { field: &'static str },
}
