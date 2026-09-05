use std::error::Error;

use chrono::Utc;
use tempfile::TempDir;
use videnoa_controller::persistence::{Database, DatabaseOptions, Store};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn administrator_credential_insert_is_singleton_and_race_safe() -> TestResult {
    // Given: two Store handles sharing one migrated SQLite database.
    let directory = TempDir::new()?;
    let database = Database::open(DatabaseOptions::new(
        directory.path().join("controller.sqlite3"),
    ))
    .await?;
    let first = Store::new(database.clone());
    let second = Store::new(database);

    // When: both attempt to create the singleton credential concurrently.
    let first_insert = first.insert_administrator_credential("$argon2id$first", Utc::now());
    let second_insert = second.insert_administrator_credential("$argon2id$second", Utc::now());
    let (first_inserted, second_inserted) = tokio::join!(first_insert, second_insert);

    // Then: exactly one insert wins and the stored value is one complete contender.
    assert_ne!(first_inserted?, second_inserted?);
    let stored = first
        .administrator_credential()
        .await?
        .ok_or_else(|| std::io::Error::other("credential row was not stored"))?;
    assert!(matches!(
        stored.expose(),
        "$argon2id$first" | "$argon2id$second"
    ));
    Ok(())
}
