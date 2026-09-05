use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;
use videnoa_controller::config::PathConfig;
use videnoa_controller::paths::{PathCapabilities, PathError};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

struct Workspace {
    directory: TempDir,
    data: PathBuf,
    paths: PathCapabilities,
}

impl Workspace {
    fn new() -> TestResult<Self> {
        let directory = TempDir::new_in(std::env::current_dir()?)?;
        let data = directory.path().join("data");
        fs::create_dir(&data)?;
        let paths = PathCapabilities::open(&PathConfig {
            input_roots: vec![directory.path().to_path_buf()],
            output_roots: vec![directory.path().to_path_buf()],
            data_root: data.clone(),
            temp_root: data.clone(),
        })?;
        Ok(Self {
            directory,
            data,
            paths,
        })
    }
}

#[test]
fn task_paths_work_without_predefined_media_directories() -> TestResult {
    // Given: an otherwise empty workspace with only Controller data and a caller's input.
    let workspace = Workspace::new()?;
    fs::write(
        workspace.directory.path().join("episode.mkv"),
        b"synthetic media",
    )?;

    // When: task-owned relative input and nested output paths are opened.
    let input = workspace.paths.open_input("episode.mkv")?;
    let output = workspace.paths.open_output("season/finished.mp4")?;

    // Then: the paths resolve in the workspace without creating generic media directories.
    assert_eq!(input.snapshot().length, 15);
    assert_eq!(
        output.display_path(),
        workspace.directory.path().join("season/finished.mp4")
    );
    assert!(!workspace.directory.path().join("input").exists());
    assert!(!workspace.directory.path().join("output").exists());
    assert!(!workspace.directory.path().join("season").exists());
    Ok(())
}

#[test]
fn controller_data_cannot_be_used_as_task_input_or_output() -> TestResult {
    // Given: private persisted state beneath the otherwise usable workspace capability.
    let workspace = Workspace::new()?;
    let database = workspace.data.join("controller.sqlite3");
    fs::write(&database, b"private synthetic state")?;

    // When: a task attempts to read or publish into Controller-owned storage.
    let input = workspace.paths.open_input(&database);
    let output = workspace
        .paths
        .open_output(workspace.data.join("injected.mp4"));
    let recovery = workspace.paths.reopen_output(&database);

    // Then: all three entry points reject private storage, retaining the original state.
    assert!(matches!(input, Err(PathError::OutsideRoots { .. })));
    assert!(matches!(output, Err(PathError::OutsideRoots { .. })));
    assert!(matches!(recovery, Err(PathError::OutsideRoots { .. })));
    assert_eq!(fs::read(database)?, b"private synthetic state");
    Ok(())
}

#[test]
fn task_relative_parent_traversal_is_rejected() -> TestResult {
    // Given: a task is restricted to the captured workspace.
    let workspace = Workspace::new()?;

    // When: a relative task path tries to leave that workspace.
    let input = workspace.paths.open_input("../outside.mkv");
    let output = workspace.paths.open_output("../outside.mp4");

    // Then: traversal is rejected rather than normalized into another directory.
    assert!(matches!(input, Err(PathError::InvalidPath { .. })));
    assert!(matches!(output, Err(PathError::InvalidPath { .. })));
    Ok(())
}

#[cfg(unix)]
#[test]
fn task_symlinks_cannot_expose_controller_data() -> TestResult {
    // Given: a workspace alias points into private Controller data.
    let workspace = Workspace::new()?;
    fs::write(
        workspace.data.join("controller.sqlite3"),
        b"private synthetic state",
    )?;
    std::os::unix::fs::symlink(&workspace.data, workspace.directory.path().join("alias"))?;

    // When: task paths attempt to use the alias for input, output, and recovery.
    let input = workspace
        .paths
        .open_input(workspace.directory.path().join("alias/controller.sqlite3"));
    let output = workspace
        .paths
        .open_output(workspace.directory.path().join("alias/output.mp4"));
    let recovery = workspace
        .paths
        .reopen_output(workspace.directory.path().join("alias/output.mp4"));

    // Then: capability traversal rejects the symlink before any state is exposed.
    assert!(matches!(input, Err(PathError::SymlinkComponent { .. })));
    assert!(matches!(output, Err(PathError::SymlinkComponent { .. })));
    assert!(matches!(recovery, Err(PathError::SymlinkComponent { .. })));
    Ok(())
}
