use std::fs;

use chrono::Utc;
use tempfile::TempDir;
use videnoa_controller::config::{ConfigBootstrap, ControllerConfig};
use videnoa_controller::persistence::{CasOutcome, Database, DatabaseOptions, Store};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn store(workspace: &TempDir) -> TestResult<Store> {
    let database = Database::open(DatabaseOptions::new(
        workspace.path().join("data/controller.sqlite3"),
    ))
    .await?;
    Ok(Store::new(database))
}

#[tokio::test]
async fn initial_toml_seeds_the_sqlite_authority() -> TestResult {
    // Given: an initialized TOML document with non-default scheduler policy.
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let changed = bootstrap
        .source()
        .replace("prefetch_per_worker = 1", "prefetch_per_worker = 9");
    fs::write(bootstrap.config_file(), changed)?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let store = store(&workspace).await?;

    // When: startup reconciles the file with a fresh migrated database.
    let config = bootstrap.reconcile(&store).await?;

    // Then: SQLite and the runtime both use the TOML scheduler seed.
    assert_eq!(config.scheduler.prefetch_per_worker, 9);
    assert_eq!(store.settings().await?.scheduler.prefetch_per_worker, 9);
    assert!(store.settings().await?.configuration_initialized);
    Ok(())
}

#[tokio::test]
async fn valid_offline_edit_is_imported_after_initialization() -> TestResult {
    // Given: an initialized database followed by a valid offline TOML edit.
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let store = store(&workspace).await?;
    bootstrap.reconcile(&store).await?;
    let original_version = store.settings().await?.version;
    let changed = bootstrap.source().replace("port = 3001", "port = 32123");
    fs::write(bootstrap.config_file(), changed)?;

    // When: the next startup reconciles the edited document.
    let edited = ConfigBootstrap::open(workspace.path())?;
    let config = edited.reconcile(&store).await?;

    // Then: the validated edit becomes the new versioned SQLite authority.
    assert_eq!(config.server.port, 32_123);
    assert_eq!(store.settings().await?.server.port, 32_123);
    assert_eq!(store.settings().await?.version, original_version + 1);
    Ok(())
}

#[tokio::test]
async fn projection_failure_leaves_a_repairable_pending_journal() -> TestResult {
    // Given: a committed configuration update whose atomic projection target is blocked.
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let store = store(&workspace).await?;
    let config = bootstrap.reconcile(&store).await?;
    let record = store.settings().await?;
    let changed_document = config.to_toml()?.replace("port = 3001", "port = 32124");
    let changed = ControllerConfig::from_toml_in(&changed_document, workspace.path())?;
    let update = changed.settings_update(record.version, &changed_document, Utc::now())?;
    assert!(matches!(
        store.update_configuration(&update).await?,
        CasOutcome::Applied { .. }
    ));
    fs::create_dir(workspace.path().join("data/.controller.toml.pending"))?;

    // When: the TOML projection fails after the authoritative commit.
    let projection = ConfigBootstrap::repair_projection(workspace.path(), &changed_document);

    // Then: failure is reported and SQLite retains the exact pending repair document.
    assert!(projection.is_err());
    let pending = store.settings().await?.pending_config_document;
    assert_eq!(pending.as_deref(), Some(changed_document.as_str()));
    Ok(())
}

#[tokio::test]
async fn stale_configuration_write_changes_neither_authority_nor_projection_journal() -> TestResult
{
    // Given: an initialized authority and an update carrying a stale future version.
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let store = store(&workspace).await?;
    let config = bootstrap.reconcile(&store).await?;
    let record = store.settings().await?;
    let changed_document = config.to_toml()?.replace("port = 3001", "port = 32125");
    let changed = ControllerConfig::from_toml_in(&changed_document, workspace.path())?;
    let update = changed.settings_update(record.version + 1, &changed_document, Utc::now())?;

    // When: the stale CAS reaches SQLite.
    let outcome = store.update_configuration(&update).await?;

    // Then: the write conflicts without changing the document or creating pending projection work.
    assert_eq!(outcome, CasOutcome::Conflict);
    let unchanged = store.settings().await?;
    assert_eq!(unchanged.version, record.version);
    assert_eq!(unchanged.config_document, record.config_document);
    assert!(unchanged.pending_config_document.is_none());
    Ok(())
}

#[tokio::test]
async fn pending_projection_repairs_before_malformed_offline_content_is_considered() -> TestResult {
    // Given: a committed pending document and a subsequently malformed TOML projection.
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let store = store(&workspace).await?;
    let config = bootstrap.reconcile(&store).await?;
    let record = store.settings().await?;
    let changed_document = config.to_toml()?.replace("port = 3001", "port = 32126");
    let changed = ControllerConfig::from_toml_in(&changed_document, workspace.path())?;
    let update = changed.settings_update(record.version, &changed_document, Utc::now())?;
    let CasOutcome::Applied { new_version } = store.update_configuration(&update).await? else {
        return Err(std::io::Error::other("configuration update conflicted").into());
    };
    fs::write(bootstrap.config_file(), "[malformed")?;

    // When: startup repairs the pending authority before opening the offline file.
    ConfigBootstrap::repair_projection(workspace.path(), &changed_document)?;
    assert!(store.complete_config_projection(new_version).await?);
    let repaired = ConfigBootstrap::open(workspace.path())?;
    let runtime = repaired.reconcile(&store).await?;

    // Then: the pending authority wins and the malformed interruption is fully repaired.
    assert_eq!(runtime.server.port, 32_126);
    assert_eq!(
        fs::read_to_string(repaired.config_file())?,
        changed_document
    );
    assert!(store.settings().await?.pending_config_document.is_none());
    Ok(())
}
