#![cfg(unix)]

use std::error::Error;
use std::os::unix::fs::symlink;

use videnoa_controller::persistence::{Database, DatabaseOptions};

#[tokio::test]
async fn redirected_database_and_sidecars_are_rejected() -> Result<(), Box<dyn Error>> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        // Given a redirected database component and an untouched destination.
        let root = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR"))?;
        let destination = root.path().join("destination");
        std::fs::write(&destination, b"protected")?;
        let database = root.path().join("controller.sqlite3");
        symlink(
            &destination,
            root.path().join(format!("controller.sqlite3{suffix}")),
        )?;

        // When opening the database, reject the redirection before SQLite runs.
        let result = Database::open(DatabaseOptions::new(database)).await;

        // Then no destination bytes were changed.
        assert!(result.is_err(), "accepted redirected component {suffix}");
        assert_eq!(std::fs::read(destination)?, b"protected");
    }
    Ok(())
}
