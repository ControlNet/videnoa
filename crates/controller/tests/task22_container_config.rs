use std::error::Error;

use tempfile::TempDir;
use videnoa_controller::config::{ConfigError, ControllerConfig};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[test]
fn explicit_missing_config_fails_instead_of_loading_defaults() -> TestResult {
    // Given: an explicit configuration path that is not mounted.
    let directory = TempDir::new()?;
    let path = directory.path().join("controller.toml");

    // When: the runtime configuration loader receives that exact path.
    let result = ControllerConfig::load(Some(&path));

    // Then: startup fails at the configuration boundary.
    assert!(matches!(
        result,
        Err(ConfigError::MissingConfigFile { path: missing }) if missing == path
    ));
    Ok(())
}
