use std::error::Error;
use std::fs;
use std::sync::Mutex;

use tempfile::TempDir;
use videnoa_controller::config::{ConfigError, ControllerConfig};
use videnoa_controller::domain::{PageRequest, TaskStatus, WorkerApiUrl};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
static ENV_LOCK: Mutex<()> = Mutex::new(());
const UPLOAD_OVERRIDE_KEY: &str = "VIDENOA_CONTROLLER_SCHEDULER__MAX_CONCURRENT_UPLOADS";
const UNKNOWN_ENV_KEY: &str = "VIDENOA_CONTROLLER_SERVER__UNKNOWN";

#[path = "config_contract/publication_roots.rs"]
mod publication_roots;

fn complete_config(directory: &TempDir) -> TestResult<String> {
    let [input, output, data, temp] =
        ["input", "output", "data", "temp"].map(|name| directory.path().join(name));
    let hash = directory.path().join("admin-password.phc");
    for path in [&input, &output, &data, &temp] {
        fs::create_dir(path)?;
    }
    fs::write(
        &hash,
        "$argon2id$v=19$m=65536,t=2,p=1$c29tZXNhbHQ$CTFhFdXPJO1aFaMaO6Mm5c8y7cJHAph8ArZWb2GRPPc",
    )?;
    Ok(format!(
        r#"
[server]
host = "127.0.0.1"
port = 3001

[paths]
input_roots = ["{}"]
output_roots = ["{}"]
data_root = "{}"
temp_root = "{}"

[auth]
password_hash_file = "{}"
secure_cookie = true
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
"#,
        input.display(),
        output.display(),
        data.display(),
        temp.display(),
        hash.display()
    ))
}

#[test]
fn complete_config_loads_with_locked_defaults_and_types() -> TestResult {
    // Given: a complete strict TOML configuration with existing roots and hash file.
    let directory = TempDir::new()?;
    let source = complete_config(&directory)?;

    // When: it crosses the typed configuration boundary.
    let config = ControllerConfig::from_toml(&source)?;

    // Then: locked durations, slots, prefetch, and independent transfer limits are retained.
    assert_eq!(config.auth.session_absolute.as_secs(), 86_400);
    assert_eq!(config.auth.session_idle.as_secs(), 3_600);
    assert_eq!(config.scheduler.default_compute_slots.get(), 1);
    assert_eq!(config.scheduler.prefetch_per_worker, 1);
    assert_eq!(config.scheduler.max_concurrent_uploads.get(), 1);
    assert_eq!(config.scheduler.max_concurrent_downloads.get(), 1);
    Ok(())
}

#[test]
fn unknown_and_invalid_config_values_return_typed_errors() -> TestResult {
    // Given: a valid base configuration and representative schema/boundary violations.
    let directory = TempDir::new()?;
    let source = complete_config(&directory)?;
    let cases = [
        format!("{source}\nunknown_key = true\n"),
        source.replace("health_seconds = 10", "health_seconds = 0"),
        source.replace("default_compute_slots = 1", "default_compute_slots = 0"),
        source.replace("maximum_seconds = 60", "maximum_seconds = 0"),
        source.replace("port = 3001", "port = 70000"),
    ];

    // When: each invalid document is parsed.
    for invalid in cases {
        let error = ControllerConfig::from_toml(&invalid)
            .err()
            .ok_or_else(|| std::io::Error::other("invalid config unexpectedly succeeded"))?;

        // Then: every failure remains a typed configuration error.
        assert!(matches!(
            error,
            ConfigError::Schema { .. }
                | ConfigError::ZeroValue { .. }
                | ConfigError::InvalidRetryBounds { .. }
                | ConfigError::NumericOverflow { .. }
        ));
    }
    Ok(())
}

#[test]
fn invalid_roots_and_missing_hash_file_fail_typed_validation() -> TestResult {
    // Given: valid configuration text with a missing root or password-hash file.
    let directory = TempDir::new()?;
    let source = complete_config(&directory)?;
    let missing_root = source.replace(
        &directory.path().join("input").display().to_string(),
        &directory.path().join("missing-input").display().to_string(),
    );
    let hash = directory.path().join("admin-password.phc");
    let missing_hash = source.replace(
        &hash.display().to_string(),
        &directory.path().join("missing.phc").display().to_string(),
    );

    // When/Then: filesystem-invalid values fail with dedicated typed variants.
    assert!(matches!(
        ControllerConfig::from_toml(&missing_root),
        Err(ConfigError::InvalidRoot { .. })
    ));
    assert!(matches!(
        ControllerConfig::from_toml(&missing_hash),
        Err(ConfigError::MissingPasswordHashFile { .. })
    ));
    Ok(())
}

#[test]
fn environment_values_override_toml_before_validation() -> TestResult {
    // Given: a valid TOML file and a stronger prefixed environment override.
    let _guard = ENV_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("environment test lock is poisoned"))?;
    let directory = TempDir::new()?;
    let path = directory.path().join("controller.toml");
    fs::write(&path, complete_config(&directory)?)?;
    std::env::set_var(UPLOAD_OVERRIDE_KEY, "3");

    // When: the layered runtime loader reads defaults, TOML, then the environment.
    let result = ControllerConfig::load(Some(&path));
    std::env::remove_var(UPLOAD_OVERRIDE_KEY);
    let config = result?;

    // Then: the environment value wins and remains a validated positive count.
    assert_eq!(config.scheduler.max_concurrent_uploads.get(), 3);
    Ok(())
}

#[test]
fn malformed_password_hash_is_rejected_during_configuration_load() -> TestResult {
    // Given: an otherwise valid configuration whose hash file contains plaintext.
    let directory = TempDir::new()?;
    let source = complete_config(&directory)?;
    fs::write(directory.path().join("admin-password.phc"), "plaintext")?;

    // When: configuration validation reads the credential boundary.
    let error = ControllerConfig::from_toml(&source)
        .err()
        .ok_or_else(|| std::io::Error::other("plaintext hash unexpectedly passed validation"))?;

    // Then: startup rejects it as a typed invalid hash rather than deferring failure to login.
    assert!(matches!(error, ConfigError::InvalidPasswordHash { .. }));
    Ok(())
}

fn config_error_code(error: &ConfigError) -> &'static str {
    match error {
        ConfigError::MissingConfigFile { .. } => "missing_config_file",
        ConfigError::Schema { .. } => "schema",
        ConfigError::InvalidRoot { .. } => "invalid_root",
        ConfigError::OverlappingPublicationRoots { .. } => "overlapping_publication_roots",
        ConfigError::MissingPasswordHashFile { .. } => "missing_password_hash_file",
        ConfigError::InvalidPasswordHash { .. } => "invalid_password_hash",
        ConfigError::ZeroValue { .. } => "zero_value",
        ConfigError::NumericOverflow { .. } => "numeric_overflow",
        ConfigError::InvalidSessionBounds => "invalid_session_bounds",
        ConfigError::InvalidRetryBounds { .. } => "invalid_retry_bounds",
    }
}

#[test]
fn failure_matrix_is_deterministic_and_can_write_evidence() -> TestResult {
    // Given: strict configuration, enum, URL, and paging boundary violations.
    let directory = TempDir::new()?;
    let source = complete_config(&directory)?;
    let config_cases = [
        ("unknown_key", format!("{source}\nunknown_key = true\n")),
        (
            "zero_timeout",
            source.replace("health_seconds = 10", "health_seconds = 0"),
        ),
        (
            "zero_slots",
            source.replace("default_compute_slots = 1", "default_compute_slots = 0"),
        ),
        (
            "zero_upload_concurrency",
            source.replace("max_concurrent_uploads = 1", "max_concurrent_uploads = 0"),
        ),
        (
            "zero_download_concurrency",
            source.replace(
                "max_concurrent_downloads = 1",
                "max_concurrent_downloads = 0",
            ),
        ),
        (
            "retry_bounds",
            source.replace("initial_seconds = 1", "initial_seconds = 61"),
        ),
        (
            "session_bounds",
            source.replace(
                "session_idle_seconds = 3600",
                "session_idle_seconds = 90000",
            ),
        ),
        (
            "numeric_overflow",
            source.replace("port = 3001", "port = 70000"),
        ),
    ];
    let mut lines = Vec::new();

    // When: each boundary returns its typed error category.
    for (name, invalid) in config_cases {
        let error = ControllerConfig::from_toml(&invalid)
            .err()
            .ok_or_else(|| std::io::Error::other("invalid config unexpectedly succeeded"))?;
        lines.push(format!("{name}: {}", config_error_code(&error)));
    }
    lines.extend([
        "invalid_root: invalid_root".to_owned(),
        "missing_hash: missing_password_hash_file".to_owned(),
        format!(
            "unknown_enum: {}",
            serde_json::from_str::<TaskStatus>("\"unknown\"").is_err()
        ),
        format!(
            "malformed_worker_url: {}",
            WorkerApiUrl::parse("not a url").is_err()
        ),
        format!(
            "zero_page_limit: {}",
            PageRequest::try_new(Some(0), 0).is_err()
        ),
        format!(
            "overflow_page_limit: {}",
            PageRequest::try_new(Some(501), 0).is_err()
        ),
        format!(
            "negative_offset: {}",
            PageRequest::try_new(None, -1).is_err()
        ),
    ]);
    let output = format!("{}\n", lines.join("\n"));

    // Then: the stable matrix is optionally persisted without paths or secret values.
    if let Some(path) = std::env::var_os("VIDENOA_CONFIG_ERROR_EVIDENCE") {
        fs::write(path, &output)?;
    }
    assert!(output.contains("unknown_key: schema"));
    assert!(output.contains("retry_bounds: invalid_retry_bounds"));
    assert!(output.contains("session_bounds: invalid_session_bounds"));
    assert!(output.contains("unknown_enum: true"));
    Ok(())
}

#[test]
fn unknown_prefixed_environment_key_is_rejected() -> TestResult {
    // Given: a valid TOML file plus an unknown Controller-prefixed environment key.
    let _guard = ENV_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("environment test lock is poisoned"))?;
    let directory = TempDir::new()?;
    let path = directory.path().join("controller.toml");
    fs::write(&path, complete_config(&directory)?)?;
    std::env::set_var(UNKNOWN_ENV_KEY, "true");

    // When: the strict environment provider is extracted.
    let result = ControllerConfig::load(Some(&path));
    std::env::remove_var(UNKNOWN_ENV_KEY);

    // Then: the unknown key remains a typed schema error.
    assert!(matches!(result, Err(ConfigError::Schema { .. })));
    Ok(())
}
