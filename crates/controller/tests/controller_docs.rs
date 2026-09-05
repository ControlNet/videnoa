use tempfile::tempdir;
use videnoa_controller::config::ControllerConfig;

#[test]
fn example_config_loads_without_operator_path_or_password_preparation(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: the shipped config and an empty synthetic workspace fixture.
    let workspace = tempdir()?;
    let source = std::fs::read_to_string("../../controller.example.toml")?;

    // When: Controller resolves the config for that workspace.
    let config = ControllerConfig::from_toml_in(&source, workspace.path())?;

    // Then: relative media paths use workspace and durable state stays under data.
    assert_eq!(
        config.paths.input_roots,
        vec![workspace.path().to_path_buf()]
    );
    assert_eq!(
        config.paths.output_roots,
        vec![workspace.path().to_path_buf()]
    );
    assert_eq!(config.paths.data_root, workspace.path().join("data"));
    assert_eq!(config.paths.temp_root, workspace.path().join("data"));
    assert!(!config.auth.secure_cookie);
    Ok(())
}

#[test]
fn example_config_exposes_only_public_runtime_sections() -> Result<(), Box<dyn std::error::Error>> {
    // Given: the shipped raw TOML document.
    let source = std::fs::read_to_string("../../controller.example.toml")?;

    // When: its top-level keys are decoded.
    let value: toml::Value = toml::from_str(&source)?;
    let mut keys = value
        .as_table()
        .ok_or_else(|| std::io::Error::other("example config must be a TOML table"))?
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();

    // Then: only settings supported by the Web UI and persisted TOML are present.
    assert_eq!(keys, ["auth", "retry", "scheduler", "server", "timeouts"]);
    assert!(!source.contains("password_hash_file"));
    assert!(!source.contains("[paths]"));
    Ok(())
}
