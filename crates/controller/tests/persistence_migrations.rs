use std::error::Error;
use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;
use tempfile::TempDir;
use videnoa_controller::persistence::{Database, DatabaseOptions};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn database_applies_migrations_and_effective_pragmas() -> TestResult {
    // Given: an empty Controller-owned SQLite path.
    let directory = TempDir::new()?;
    let path = directory.path().join("controller.sqlite3");

    // When: the durable database is opened.
    let database = Database::open(
        DatabaseOptions::new(&path)
            .with_max_connections(4)
            .with_busy_timeout(Duration::from_millis(2_500)),
    )
    .await?;

    // Then: only the planned tables plus SQLx metadata exist and every connection is configured.
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
            .fetch_all(database.pool())
            .await?;
    assert_eq!(
        tables,
        [
            "_sqlx_migrations",
            "auth_sessions",
            "controller_settings",
            "task_attempts",
            "task_idempotency",
            "tasks",
            "workers",
        ]
    );
    let pragma = sqlx::query(
        "SELECT (SELECT journal_mode FROM pragma_journal_mode) AS journal_mode, \
                (SELECT foreign_keys FROM pragma_foreign_keys) AS foreign_keys, \
                (SELECT timeout FROM pragma_busy_timeout) AS busy_timeout",
    )
    .fetch_one(database.pool())
    .await?;
    assert_eq!(pragma.try_get::<String, _>("journal_mode")?, "wal");
    assert_eq!(pragma.try_get::<i64, _>("foreign_keys")?, 1);
    assert_eq!(pragma.try_get::<i64, _>("busy_timeout")?, 2_500);
    assert_eq!(database.pool().options().get_max_connections(), 4);
    Ok(())
}

#[tokio::test]
async fn existing_database_migrates_idempotently() -> TestResult {
    // Given: a database that has already completed Controller migrations.
    let directory = TempDir::new()?;
    let path = directory.path().join("controller.sqlite3");
    Database::open(DatabaseOptions::new(&path))
        .await?
        .close()
        .await;

    // When: the same database is opened again.
    let database = Database::open(DatabaseOptions::new(&path)).await?;

    // Then: SQLx records all production migrations and the singleton settings row remains unique.
    let migration_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(database.pool())
            .await?;
    let settings_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM controller_settings")
        .fetch_one(database.pool())
        .await?;
    assert_eq!(migration_count, 4);
    assert_eq!(settings_count, 1);
    Ok(())
}

#[tokio::test]
async fn invalid_sqlx_migration_rolls_back_its_partial_schema() -> TestResult {
    // Given: one controlled SQLx migration containing valid DDL followed by invalid SQL.
    let directory = TempDir::new()?;
    let migrations = directory.path().join("migrations");
    std::fs::create_dir(&migrations)?;
    std::fs::write(
        migrations.join("0001_invalid.sql"),
        "CREATE TABLE rolled_back (id INTEGER PRIMARY KEY);\nTHIS IS INVALID SQL;\n",
    )?;
    let database_path = directory.path().join("invalid.sqlite3");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(database_path)
                .create_if_missing(true),
        )
        .await?;
    let migrator = Migrator::new(migrations).await?;

    // When: SQLx applies the invalid migration.
    let result = migrator.run(&pool).await;

    // Then: the migration fails and its preceding table creation is absent.
    assert!(result.is_err());
    let table_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = 'rolled_back'",
    )
    .fetch_one(&pool)
    .await?;
    let applied_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&pool)
            .await?;
    assert_eq!(table_count, 0);
    assert_eq!(applied_count, 0);
    Ok(())
}
