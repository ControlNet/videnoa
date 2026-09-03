use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
const NPM_BIN: &str = "npm.cmd";

#[cfg(not(windows))]
const NPM_BIN: &str = "npm";

fn run_npm(web_directory: &Path, arguments: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new(NPM_BIN)
        .args(arguments)
        .current_dir(web_directory)
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "npm {} exited with status {status}",
            arguments.join(" ")
        ))
        .into())
    }
}

fn controller_web_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../controller-web")
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("PROFILE").as_deref() != Ok("release") {
        return Ok(());
    }

    for path in [
        "../../controller-web/src",
        "../../controller-web/index.html",
        "../../controller-web/package.json",
        "../../controller-web/package-lock.json",
        "../../controller-web/tsconfig.json",
        "../../controller-web/tsconfig.app.json",
        "../../controller-web/tsconfig.node.json",
        "../../controller-web/vite.config.ts",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    let web_directory = controller_web_directory();
    if !web_directory.is_dir() {
        return Err(std::io::Error::other(format!(
            "Controller frontend directory is missing: {}",
            web_directory.display()
        ))
        .into());
    }

    if std::env::var_os("VIDENOA_CONTROLLER_WEB_PREBUILT").is_none() {
        run_npm(&web_directory, &["ci", "--no-fund"])?;
        run_npm(&web_directory, &["run", "build"])?;
    }

    let index_path = web_directory.join("dist/index.html");
    if !index_path.is_file() {
        return Err(std::io::Error::other(format!(
            "Controller frontend build did not produce {}",
            index_path.display()
        ))
        .into());
    }

    Ok(())
}
