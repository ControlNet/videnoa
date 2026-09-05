use std::fs;

use tempfile::TempDir;
use videnoa_controller::config::{ConfigError, ControllerConfig};

use super::{complete_config, TestResult};

#[test]
fn temp_root_overlapping_an_output_root_is_rejected() -> TestResult {
    // Given: temp storage and output storage contain one another in either direction.
    let directory = TempDir::new()?;
    let source = complete_config(&directory)?;
    let output = directory.path().join("output");
    let overlapping_temp = output.join("controller-temp");
    fs::create_dir(&overlapping_temp)?;
    let temp_inside_output = source.replace(
        &directory.path().join("temp").display().to_string(),
        &overlapping_temp.display().to_string(),
    );
    let temp = directory.path().join("temp");
    let overlapping_output = temp.join("published");
    fs::create_dir(&overlapping_output)?;
    let output_inside_temp = source.replace(
        &output.display().to_string(),
        &overlapping_output.display().to_string(),
    );

    // When: configuration crosses the filesystem boundary validator.
    let results = [
        ControllerConfig::from_toml(&temp_inside_output),
        ControllerConfig::from_toml(&output_inside_temp),
    ];

    // Then: startup rejects the overlap instead of exposing temporary bytes as outputs.
    assert!(results
        .into_iter()
        .all(|result| matches!(result, Err(ConfigError::OverlappingPublicationRoots { .. }))));
    Ok(())
}
