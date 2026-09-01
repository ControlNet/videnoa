use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    targets: Vec<CargoTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn package_and_binary_are_named_videnoa_controller() -> TestResult {
    // Given: Cargo's package and integration-test binary environment.
    let binary = Path::new(env!("CARGO_BIN_EXE_videnoa-controller"));

    // When: their identities are inspected.
    let binary_stem = binary
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| std::io::Error::other("Controller binary has no UTF-8 file stem"))?;

    // Then: both public identities use the locked product name.
    assert_eq!(env!("CARGO_PKG_NAME"), "videnoa-controller");
    assert_eq!(binary_stem, "videnoa-controller");
    Ok(())
}

#[test]
fn workspace_keeps_existing_products_and_adds_controller() -> TestResult {
    // Given: Cargo metadata for the repository workspace.
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(workspace_root())
        .output()?;
    assert!(output.status.success(), "cargo metadata failed");

    // When: workspace package and target identities are decoded.
    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)?;
    let workspace_packages: Vec<&CargoPackage> = metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .collect();

    // Then: existing products retain their names and Controller is independent.
    for expected in [
        "videnoa-app",
        "videnoa-core",
        "videnoa-desktop",
        "videnoa-controller",
    ] {
        assert!(workspace_packages
            .iter()
            .any(|package| package.name == expected));
    }

    let controller = workspace_packages
        .iter()
        .find(|package| package.name == "videnoa-controller")
        .ok_or_else(|| std::io::Error::other("Controller package missing from workspace"))?;
    assert!(controller.targets.iter().any(|target| {
        target.name == "videnoa-controller" && target.kind.iter().any(|kind| kind == "bin")
    }));
    Ok(())
}

#[test]
fn dependency_tree_excludes_gpu_and_model_runtime_crates() -> TestResult {
    // Given: the complete normal and build dependency tree for Controller.
    let output = Command::new(env!("CARGO"))
        .args([
            "tree",
            "-p",
            "videnoa-controller",
            "--edges",
            "normal,build",
        ])
        .current_dir(workspace_root())
        .output()?;
    assert!(output.status.success(), "cargo tree failed");
    let tree = String::from_utf8(output.stdout)?.to_ascii_lowercase();

    // When: forbidden GPU and model-runtime package names are checked.
    for forbidden in ["videnoa-core", "ort ", "cuda", "cudnn", "tensorrt"] {
        // Then: no direct or transitive Controller dependency contains them.
        assert!(
            !tree.contains(forbidden),
            "forbidden dependency found: {forbidden}\n{tree}"
        );
    }
    Ok(())
}
