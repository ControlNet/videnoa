use std::fs;

use anyhow::{Context, Result};
use tempfile::tempdir;
use videnoa_controller::auth::hash_password;
use videnoa_controller::config::ControllerConfig;

#[test]
fn example_config_loads_when_operator_paths_exist() -> Result<()> {
    let root = tempdir()?;
    let input = root.path().join("input");
    let output = root.path().join("output");
    let data = root.path().join("data");
    let temp = root.path().join("temp");
    let hash = root.path().join("admin-password.phc");
    for directory in [&input, &output, &data, &temp] {
        fs::create_dir(directory)?;
    }
    fs::write(&hash, hash_password("ephemeral-docs-test-password")?)?;

    let source = fs::read_to_string("../../controller.example.toml")?
        .replace("/srv/media/incoming", &path_text(&input)?)
        .replace("/srv/media/library", &path_text(&output)?)
        .replace("/var/lib/videnoa-controller/temp", &path_text(&temp)?)
        .replace(
            "/var/lib/videnoa-controller/admin-password.phc",
            &path_text(&hash)?,
        )
        .replace("/var/lib/videnoa-controller", &path_text(&data)?);

    let config = ControllerConfig::from_toml(&source)?;
    assert_eq!(config.paths.input_roots, vec![input]);
    assert_eq!(config.paths.output_roots, vec![output]);
    assert_eq!(config.paths.data_root, data);
    assert_eq!(config.paths.temp_root, temp);
    assert_eq!(config.auth.password_hash_file, hash);
    Ok(())
}

fn path_text(path: &std::path::Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .context("temporary path must be UTF-8")
}
