use std::path::Path;
use std::process::Command;

#[cfg(windows)]
const NPM_BIN: &str = "npm.cmd";

#[cfg(not(windows))]
const NPM_BIN: &str = "npm";

fn build_frontend_assets_for_release() {
    let profile = std::env::var("PROFILE").unwrap_or_default();
    if profile != "release" {
        return;
    }

    println!("cargo:rerun-if-changed=../../web/src");
    println!("cargo:rerun-if-changed=../../web/index.html");
    println!("cargo:rerun-if-changed=../../web/package.json");
    println!("cargo:rerun-if-changed=../../web/package-lock.json");
    println!("cargo:rerun-if-changed=../../web/vite.config.ts");
    println!("cargo:rerun-if-changed=../../web/vitest.config.ts");
    println!("cargo:rerun-if-changed=../../web/tsconfig.json");

    let web_dir = Path::new("../../web");
    if !web_dir.is_dir() {
        panic!("frontend directory missing: {}", web_dir.to_string_lossy());
    }

    let lockfile_exists = web_dir.join("package-lock.json").exists();
    let install_args: [&str; 2] = if lockfile_exists {
        ["ci", "--no-fund"]
    } else {
        ["install", "--no-fund"]
    };

    println!(
        "cargo:warning=Installing frontend dependencies (npm {} )...",
        install_args[0]
    );

    let install_status = Command::new(NPM_BIN)
        .args(install_args)
        .current_dir(web_dir)
        .status()
        .expect("Failed to execute npm install step. Is npm installed and available in PATH?");

    if !install_status.success() {
        panic!(
            "Frontend dependency install failed (npm {} exited with non-zero status)",
            install_args[0]
        );
    }

    println!("cargo:warning=Building frontend (npm run build)...");

    let build_status = Command::new(NPM_BIN)
        .args(["run", "build"])
        .current_dir(web_dir)
        .status()
        .expect("Failed to execute `npm run build`. Is npm installed and available in PATH?");

    if !build_status.success() {
        panic!("Frontend build failed (npm run build exited with non-zero status)");
    }

    let dist_dir = web_dir.join("dist");
    if !dist_dir.is_dir() {
        panic!(
            "Frontend build succeeded but dist directory is missing: {}",
            dist_dir.to_string_lossy()
        );
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=proto/onnx.proto3");
    println!("cargo:rerun-if-changed=proto/");

    build_frontend_assets_for_release();
    prost_build::compile_protos(&["proto/onnx.proto3"], &["proto/"])?;
    Ok(())
}
