use std::fs;

use chrono::Utc;
use tempfile::TempDir;
use videnoa_controller::config::{ConfigBootstrap, SettingsUpdate};
use videnoa_controller::persistence::{CasOutcome, Database, DatabaseOptions, Store};
use videnoa_controller::scheduler::Scheduler;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn store(workspace: &TempDir) -> TestResult<Store> {
    Ok(Store::new(
        Database::open(DatabaseOptions::new(
            workspace.path().join("data/controller.sqlite3"),
        ))
        .await?,
    ))
}

#[tokio::test]
async fn toml_is_authority_even_when_legacy_sqlite_disagrees() -> TestResult {
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let store = store(&workspace).await?;
    sqlx::query(
        "UPDATE controller_settings SET paused = 1, prefetch_per_worker = 99,
        server_port = 43210, configuration_initialized = 1, config_document = '[invalid',
        pending_config_document = '[invalid'",
    )
    .execute(store.database().pool())
    .await?;
    bootstrap.initialize(&store)?;
    assert_eq!(store.config_manager().config(), *bootstrap.config());
    assert!(!store.config_manager().scheduler().paused);
    let scheduler = Scheduler::load(store.clone())?;
    assert!(
        scheduler
            .allows(videnoa_controller::lifecycle::DurableAction::Submit)
            .await?
    );
    let legacy: i64 = sqlx::query_scalar("SELECT prefetch_per_worker FROM controller_settings")
        .fetch_one(store.database().pool())
        .await?;
    assert_eq!(legacy, 99);
    Ok(())
}

#[tokio::test]
async fn manual_edit_is_inert_until_restart_then_loads_directly() -> TestResult {
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let running = store(&workspace).await?;
    bootstrap.initialize(&running)?;
    fs::write(
        bootstrap.config_file(),
        bootstrap.source().replace("port = 3001", "port = 32123"),
    )?;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(running.config_manager().config().server.port, 3001);
    let restarted = store(&workspace).await?;
    ConfigBootstrap::open(workspace.path())?.initialize(&restarted)?;
    assert_eq!(restarted.config_manager().config().server.port, 32123);
    Ok(())
}

#[tokio::test]
async fn pause_persists_only_in_toml_and_stale_generation_conflicts() -> TestResult {
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let running = store(&workspace).await?;
    bootstrap.initialize(&running)?;
    let record = running.config_manager().settings()?;
    let mut update = SettingsUpdate {
        expected_version: record.version,
        scheduler: record.scheduler,
        timeouts: record.timeouts,
        retry: record.retry,
        updated_at: Utc::now(),
    };
    update.scheduler.paused = true;
    assert!(matches!(
        running.config_manager().update_settings(&update).await?,
        CasOutcome::Applied { .. }
    ));
    let saved = fs::read_to_string(bootstrap.config_file())?;
    assert!(saved.contains("paused = true"));
    assert_eq!(
        running.config_manager().update_settings(&update).await?,
        CasOutcome::Conflict
    );
    assert_eq!(fs::read_to_string(bootstrap.config_file())?, saved);
    let legacy: (i64, i64, String, Option<String>) = sqlx::query_as(
        "SELECT paused, configuration_initialized, config_document, pending_config_document FROM controller_settings"
    ).fetch_one(running.database().pool()).await?;
    assert_eq!(legacy, (0, 0, String::new(), None));
    let restarted = store(&workspace).await?;
    ConfigBootstrap::open(workspace.path())?.initialize(&restarted)?;
    assert!(restarted.config_manager().scheduler().paused);
    Ok(())
}

#[tokio::test]
async fn crash_after_toml_write_requires_no_database_repair() -> TestResult {
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let running = store(&workspace).await?;
    bootstrap.initialize(&running)?;
    let mut changed = bootstrap.config().clone();
    changed.scheduler.prefetch_per_worker = 9;
    // Simulate interruption after the durable rename, before any runtime application.
    bootstrap.persist(&changed.to_toml()?)?;
    assert_eq!(running.config_manager().scheduler().prefetch_per_worker, 1);
    let restarted = store(&workspace).await?;
    ConfigBootstrap::open(workspace.path())?.initialize(&restarted)?;
    assert_eq!(
        restarted.config_manager().scheduler().prefetch_per_worker,
        9
    );
    Ok(())
}

#[tokio::test]
async fn graceful_shutdown_preserves_manual_toml_edits_for_restart() -> TestResult {
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let running = store(&workspace).await?;
    bootstrap.initialize(&running)?;
    let scheduler = Scheduler::load(running.clone())?;
    let edited = bootstrap.source().replace("port = 3001", "port = 32127");
    fs::write(bootstrap.config_file(), &edited)?;
    videnoa_controller::recovery::ShutdownCoordinator::new()
        .shutdown(&scheduler, Utc::now(), std::time::Duration::from_secs(1))
        .await?;
    assert!(running.config_manager().scheduler().paused);
    assert_eq!(fs::read_to_string(bootstrap.config_file())?, edited);
    let restarted = store(&workspace).await?;
    ConfigBootstrap::open(workspace.path())?.initialize(&restarted)?;
    assert_eq!(restarted.config_manager().config().server.port, 32127);
    assert!(!restarted.config_manager().scheduler().paused);
    Ok(())
}

#[test]
fn restart_migrates_data_root_once_and_recovers_without_ephemeral_state() -> TestResult {
    // Given: an initialized Controller with durable state and newly configured roots.
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let original_root = workspace.path().join("data");
    let moved_root = workspace.path().join("moved-data");
    let cache_root = workspace.path().join("media/.videnoa-cache");
    fs::write(original_root.join("durable-state"), "before migration")?;
    let mut changed = bootstrap.config().clone();
    changed.paths.data_root.clone_from(&moved_root);
    changed.paths.temp_root.clone_from(&cache_root);
    bootstrap.persist(&changed.to_toml()?)?;

    // When: startup applies the pending root change.
    let migrated = ConfigBootstrap::open(workspace.path())?;

    // Then: configuration and durable state live under DATA ROOT, and CACHE ROOT is ready.
    assert_eq!(migrated.config_file(), moved_root.join("controller.toml"));
    assert_eq!(migrated.config().paths.data_root, moved_root);
    assert_eq!(migrated.config().paths.temp_root, cache_root);
    assert!(migrated.retained_private_roots().contains(&original_root));
    assert_eq!(
        fs::read_to_string(moved_root.join("durable-state"))?,
        "before migration"
    );
    assert!(cache_root.is_dir());

    // When: the bootstrap locator is lost with an ephemeral container layer.
    fs::write(moved_root.join("durable-state"), "after migration")?;
    let current_config = fs::read_to_string(moved_root.join("controller.toml"))?;
    fs::write(
        moved_root.join("controller.toml"),
        current_config.replace("port = 3001", "port = 32123"),
    )?;
    fs::remove_file(original_root.join(".videnoa-data-root.toml"))?;
    let recovered = ConfigBootstrap::open(workspace.path())?;

    // Then: the activated destination is rediscovered without overwriting newer state.
    assert_eq!(recovered.config_file(), moved_root.join("controller.toml"));
    assert_eq!(recovered.config().server.port, 32123);
    assert!(recovered.retained_private_roots().contains(&original_root));
    assert_eq!(
        fs::read_to_string(moved_root.join("durable-state"))?,
        "after migration"
    );

    // When: DATA ROOT is moved again.
    let second_root = workspace.path().join("second-data");
    let mut second_config = recovered.config().clone();
    second_config.paths.data_root.clone_from(&second_root);
    recovered.persist(&second_config.to_toml()?)?;
    let moved_again = ConfigBootstrap::open(workspace.path())?;

    // Then: every retained root remains classified as private Controller storage.
    assert_eq!(
        moved_again.config_file(),
        second_root.join("controller.toml")
    );
    assert!(moved_again
        .retained_private_roots()
        .contains(&original_root));
    assert!(moved_again.retained_private_roots().contains(&moved_root));
    assert_eq!(
        fs::read_to_string(second_root.join("durable-state"))?,
        "after migration"
    );
    Ok(())
}

#[test]
fn restart_rejects_a_data_root_nested_inside_the_active_root() -> TestResult {
    // Given: a configured destination nested inside the directory being copied.
    let workspace = TempDir::new()?;
    let bootstrap = ConfigBootstrap::open(workspace.path())?;
    let mut changed = bootstrap.config().clone();
    changed.paths.data_root = workspace.path().join("data/nested");
    bootstrap.persist(&changed.to_toml()?)?;

    // When: startup tries to activate the nested destination.
    let result = ConfigBootstrap::open(workspace.path());

    // Then: migration stops before recursive self-copy can begin.
    assert!(matches!(
        result,
        Err(videnoa_controller::config::ConfigError::InvalidRoot {
            field: "paths.data_root",
            ..
        })
    ));
    assert!(!workspace.path().join("data/nested").exists());
    Ok(())
}
