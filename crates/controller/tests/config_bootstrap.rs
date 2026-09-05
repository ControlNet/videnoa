use std::fs;

#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

use tempfile::TempDir;
use videnoa_controller::config::{ConfigBootstrap, ControllerConfig};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[test]
fn missing_configuration_initializes_only_the_data_directory() -> TestResult {
    // Given: an empty workspace.
    let workspace = TempDir::new()?;

    // When: the controller configuration is bootstrapped.
    let bootstrap = ConfigBootstrap::open(workspace.path())?;

    // Then: defaults are projected and no legacy media/auth directories are created.
    assert_eq!(
        bootstrap.config(),
        &ControllerConfig::for_workspace(workspace.path())?
    );
    assert!(workspace.path().join("data/controller.toml").is_file());
    assert!(workspace.path().join("data").is_dir());
    assert!(!workspace.path().join("input").exists());
    assert!(!workspace.path().join("output").exists());
    assert!(!workspace.path().join("auth").exists());
    Ok(())
}

#[test]
fn whitespace_configuration_is_initialized_but_malformed_content_is_preserved() -> TestResult {
    // Given: one whitespace-only file and one malformed non-empty file.
    let empty_workspace = TempDir::new()?;
    fs::create_dir(empty_workspace.path().join("data"))?;
    fs::write(empty_workspace.path().join("data/controller.toml"), " \n\t")?;
    let malformed_workspace = TempDir::new()?;
    fs::create_dir(malformed_workspace.path().join("data"))?;
    let malformed_path = malformed_workspace.path().join("data/controller.toml");
    fs::write(&malformed_path, "[server\ninvalid")?;

    // When: both workspaces cross the bootstrap boundary.
    ConfigBootstrap::open(empty_workspace.path())?;
    let error = ConfigBootstrap::open(malformed_workspace.path()).err();

    // Then: whitespace becomes defaults while malformed user input is not overwritten.
    assert!(
        empty_workspace
            .path()
            .join("data/controller.toml")
            .metadata()?
            .len()
            > 3
    );
    assert!(error.is_some());
    assert_eq!(fs::read_to_string(malformed_path)?, "[server\ninvalid");
    Ok(())
}

#[test]
fn existing_configuration_loads_all_policy_fields() -> TestResult {
    // Given: a valid policy-only configuration.
    let workspace = TempDir::new()?;
    fs::create_dir(workspace.path().join("data"))?;
    fs::write(
        workspace.path().join("data/controller.toml"),
        r#"
[server]
host = "127.0.0.1"
port = 43123

[auth]
secure_cookie = true
session_absolute_seconds = 7200
session_idle_seconds = 600

[scheduler]
paused = true
default_compute_slots = 2
prefetch_per_worker = 3
max_concurrent_uploads = 4
max_concurrent_downloads = 5

[timeouts]
health_seconds = 11
poll_seconds = 12
transfer_seconds = 13

[retry]
initial_seconds = 2
maximum_seconds = 20
max_attempts = 7
"#,
    )?;

    // When: the existing document is loaded.
    let bootstrap = ConfigBootstrap::open(workspace.path())?;

    // Then: public fields and derived paths reflect the document and workspace.
    assert_eq!(bootstrap.config().server.port, 43_123);
    assert!(bootstrap.config().auth.secure_cookie);
    assert!(bootstrap.config().scheduler.paused);
    assert_eq!(bootstrap.config().timeouts.poll.as_secs(), 12);
    assert_eq!(bootstrap.config().retry.max_attempts.get(), 7);
    assert_eq!(
        bootstrap.config().paths.input_roots,
        [workspace.path().canonicalize()?]
    );
    assert_eq!(
        bootstrap.config().paths.data_root,
        workspace.path().join("data")
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn external_data_symlink_is_rejected_without_writing_outside() -> TestResult {
    // Given: the workspace data entry redirects to an external directory.
    let workspace = TempDir::new()?;
    let external = TempDir::new()?;
    symlink(external.path(), workspace.path().join("data"))?;

    // When: configuration bootstrap crosses the workspace boundary.
    let result = ConfigBootstrap::open(workspace.path());

    // Then: bootstrap rejects the redirect and creates no external configuration.
    assert!(result.is_err());
    assert!(!external.path().join("controller.toml").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn external_configuration_symlink_is_rejected_without_reading_target() -> TestResult {
    // Given: the projected configuration redirects to a valid external document.
    let workspace = TempDir::new()?;
    let external = TempDir::new()?;
    fs::create_dir(workspace.path().join("data"))?;
    let external_config = external.path().join("controller.toml");
    fs::write(
        &external_config,
        ControllerConfig::for_workspace(workspace.path())?.to_toml()?,
    )?;
    symlink(
        &external_config,
        workspace.path().join("data/controller.toml"),
    )?;

    // When: configuration bootstrap attempts to load the projection.
    let result = ConfigBootstrap::open(workspace.path());

    // Then: bootstrap rejects the redirect without replacing the external document.
    assert!(result.is_err());
    assert!(!fs::read_to_string(external_config)?.is_empty());
    Ok(())
}

#[cfg(unix)]
#[test]
fn external_pending_projection_symlink_is_rejected_without_truncating_target() -> TestResult {
    // Given: the atomic projection staging path redirects to an external file.
    let workspace = TempDir::new()?;
    let external = TempDir::new()?;
    fs::create_dir(workspace.path().join("data"))?;
    let external_file = external.path().join("sentinel");
    fs::write(&external_file, "preserve-me")?;
    symlink(
        &external_file,
        workspace.path().join("data/.controller.toml.pending"),
    )?;

    // When: durable configuration repair attempts an atomic projection.
    let result = ConfigBootstrap::persist_document(workspace.path(), "[server]\nport = 3001\n");

    // Then: projection fails without following or truncating the external target.
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(external_file)?, "preserve-me");
    Ok(())
}

#[cfg(unix)]
#[test]
fn bootstrap_creates_and_tightens_private_configuration_permissions() -> TestResult {
    // Given: a workspace with an overly broad pre-existing data directory and empty config.
    let workspace = TempDir::new()?;
    let data_root = workspace.path().join("data");
    fs::create_dir(&data_root)?;
    fs::set_permissions(&data_root, fs::Permissions::from_mode(0o755))?;
    let config_file = data_root.join("controller.toml");
    fs::write(&config_file, "")?;
    fs::set_permissions(&config_file, fs::Permissions::from_mode(0o644))?;

    // When: configuration bootstrap prepares and initializes the workspace.
    ConfigBootstrap::open(workspace.path())?;

    // Then: both controller-owned paths are private to the runtime user.
    assert_eq!(fs::metadata(data_root)?.mode() & 0o777, 0o700);
    assert_eq!(fs::metadata(config_file)?.mode() & 0o777, 0o600);
    Ok(())
}
