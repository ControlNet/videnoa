use std::error::Error;

use tempfile::TempDir;
use videnoa_controller::config::{ConfigError, ControllerConfig};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn complete_config() -> &'static str {
    r#"
[server]
host = "127.0.0.1"
port = 3001

[auth]
secure_cookie = false
session_absolute_seconds = 86400
session_idle_seconds = 3600

[scheduler]
paused = false
default_compute_slots = 1
prefetch_per_worker = 1
max_concurrent_uploads = 1
max_concurrent_downloads = 1

[timeouts]
health_seconds = 10
poll_seconds = 5
transfer_seconds = 300

[retry]
initial_seconds = 1
maximum_seconds = 60
max_attempts = 5
"#
}

#[test]
fn policy_only_config_loads_with_workspace_derived_paths() -> TestResult {
    // Given: a complete public policy document and an existing workspace.
    let workspace = TempDir::new()?;

    // When: it crosses the typed configuration boundary.
    let config = ControllerConfig::from_toml_in(complete_config(), workspace.path())?;

    // Then: policy values load and paths derive exclusively from the workspace.
    assert_eq!(config.server.port, 3001);
    assert!(!config.auth.secure_cookie);
    assert_eq!(config.scheduler.default_compute_slots.get(), 1);
    assert_eq!(config.paths.input_roots, [workspace.path()]);
    assert_eq!(config.paths.output_roots, [workspace.path()]);
    assert_eq!(config.paths.data_root, workspace.path().join("data"));
    assert_eq!(config.paths.temp_root, workspace.path().join("data"));
    Ok(())
}

#[test]
fn paths_and_password_fields_are_rejected() {
    // Given: legacy user-defined filesystem fields.
    let legacy = format!(
        "{}\n[paths]\ninput_roots = [\"input\"]\n",
        complete_config()
    );
    let password = complete_config().replace(
        "secure_cookie = false",
        "password_hash_file = \"secret.phc\"\nsecure_cookie = false",
    );

    // When: either document crosses the strict boundary.
    let legacy_result = ControllerConfig::from_toml(&legacy);
    let password_result = ControllerConfig::from_toml(&password);

    // Then: neither legacy capability nor credential path becomes runtime input.
    assert!(matches!(legacy_result, Err(ConfigError::Schema { .. })));
    assert!(matches!(password_result, Err(ConfigError::Schema { .. })));
}

#[test]
fn invalid_public_values_return_typed_errors() {
    // Given: representative schema and value boundary violations.
    let cases = [
        format!("{}\nunknown_key = true\n", complete_config()),
        complete_config().replace("health_seconds = 10", "health_seconds = 0"),
        complete_config().replace("default_compute_slots = 1", "default_compute_slots = 0"),
        complete_config().replace("port = 3001", "port = 70000"),
        complete_config().replace("initial_seconds = 1", "initial_seconds = 61"),
        complete_config().replace(
            "session_idle_seconds = 3600",
            "session_idle_seconds = 90000",
        ),
    ];

    // When/Then: every invalid document fails at the typed boundary.
    for source in cases {
        assert!(ControllerConfig::from_toml(&source).is_err());
    }
}

#[test]
fn serialization_roundtrip_preserves_every_public_field() -> TestResult {
    // Given: a non-default valid configuration.
    let source = complete_config()
        .replace("port = 3001", "port = 43210")
        .replace("paused = false", "paused = true")
        .replace("max_attempts = 5", "max_attempts = 9");
    let config = ControllerConfig::from_toml(&source)?;

    // When: it is projected and parsed again.
    let projected = config.to_toml()?;
    let reloaded = ControllerConfig::from_toml(&projected)?;

    // Then: the typed public policy is unchanged and no private paths are serialized.
    assert_eq!(reloaded, config);
    assert!(!projected.contains("paths"));
    assert!(!projected.contains("password"));
    Ok(())
}
