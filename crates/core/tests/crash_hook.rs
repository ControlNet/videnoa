use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tempfile::tempdir;
use videnoa_core::logging::{
    install_panic_hook, PanicHookInstallPlan, DEFAULT_CRASH_DIR_NAME, DEFAULT_LOG_DIR_NAME,
};

fn run_panic_child(mode: &str, data_dir: &Path) -> std::process::Output {
    Command::new(std::env::current_exe().expect("test executable path"))
        .arg("panic_hook_child_entrypoint")
        .arg("--exact")
        .arg("--nocapture")
        .env("VIDENOA_PANIC_CHILD_MODE", mode)
        .env("VIDENOA_PANIC_CHILD_DATA_DIR", data_dir)
        .output()
        .expect("run panic hook child")
}

fn collect_crash_artifacts(crash_dir: &Path) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(crash_dir)
        .expect("read crash directory")
        .map(|entry| entry.expect("read crash directory entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("log"))
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
fn panic_hook_child_entrypoint() {
    let Ok(mode) = std::env::var("VIDENOA_PANIC_CHILD_MODE") else {
        return;
    };

    let data_dir = PathBuf::from(
        std::env::var("VIDENOA_PANIC_CHILD_DATA_DIR")
            .expect("VIDENOA_PANIC_CHILD_DATA_DIR must be set"),
    );

    let first_install = install_panic_hook(Some(data_dir.as_path()));
    assert!(matches!(
        first_install,
        PanicHookInstallPlan::Installed { .. } | PanicHookInstallPlan::AlreadyInstalled { .. }
    ));

    let second_install = install_panic_hook(Some(data_dir.as_path()));
    assert!(matches!(
        second_install,
        PanicHookInstallPlan::AlreadyInstalled { .. }
    ));

    match mode.as_str() {
        "write_success" => panic!("intentional panic for crash_hook_writes_crash_file"),
        "write_failure" => {
            let crash_dir = data_dir
                .join(DEFAULT_LOG_DIR_NAME)
                .join(DEFAULT_CRASH_DIR_NAME);
            if crash_dir.exists() {
                fs::remove_dir_all(&crash_dir).expect("remove crash directory");
            }
            fs::write(&crash_dir, b"not-a-directory").expect("replace crash directory with file");
            panic!("intentional panic for crash_hook_unwritable_crash_dir");
        }
        other => panic!("unknown panic hook child mode: {other}"),
    }
}

#[test]
fn crash_hook_writes_crash_file() {
    let data_dir = tempdir().expect("tempdir");
    let output = run_panic_child("write_success", data_dir.path());

    assert!(!output.status.success(), "child process should panic");

    let crash_dir = data_dir
        .path()
        .join(DEFAULT_LOG_DIR_NAME)
        .join(DEFAULT_CRASH_DIR_NAME);
    let crash_artifacts = collect_crash_artifacts(&crash_dir);
    assert!(!crash_artifacts.is_empty(), "expected crash artifact");

    let newest_artifact = crash_artifacts.last().expect("artifact path");
    let contents = fs::read_to_string(newest_artifact).expect("read crash artifact");

    assert!(contents.contains("timestamp_utc="));
    assert!(contents.contains("payload=intentional panic for crash_hook_writes_crash_file"));
    assert!(contents.contains("location="));
    assert!(contents.contains("backtrace_policy="));
    assert!(contents.contains("backtrace:"));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("thread panicked while panicking"));
}

#[test]
fn crash_hook_unwritable_crash_dir_warns_and_does_not_repanic() {
    let data_dir = tempdir().expect("tempdir");
    let output = run_panic_child("write_failure", data_dir.path());

    assert!(!output.status.success(), "child process should panic");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("intentional panic for crash_hook_unwritable_crash_dir"));
    assert!(stderr.contains("Warning: failed to write panic crash artifact under"));
    assert!(!stderr.contains("thread panicked while panicking"));
}
