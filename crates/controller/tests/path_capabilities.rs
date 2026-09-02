use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use tempfile::TempDir;
use videnoa_controller::config::PathConfig;
use videnoa_controller::paths::{PathCapabilities, PathError};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

struct Fixture {
    _directory: TempDir,
    input: PathBuf,
    output: PathBuf,
    outside: PathBuf,
    capabilities: PathCapabilities,
}

impl Fixture {
    fn new() -> TestResult<Self> {
        let directory = TempDir::new()?;
        let input = directory.path().join("input");
        let output = directory.path().join("output");
        let outside = directory.path().join("outside");
        let data = directory.path().join("data");
        let temp = data.join("temp");
        for path in [&input, &output, &outside, &data, &temp] {
            fs::create_dir(path)?;
        }
        let capabilities = PathCapabilities::open(&PathConfig {
            input_roots: vec![input.clone()],
            output_roots: vec![output.clone()],
            data_root: data,
            temp_root: temp,
        })?;
        Ok(Self {
            _directory: directory,
            input,
            output,
            outside,
            capabilities,
        })
    }
}

#[test]
fn relative_roots_resolve_from_the_process_directory() -> TestResult {
    // Given: configured roots expressed relative to the Controller process directory.
    let process_directory = std::env::current_dir()?;
    let directory = TempDir::new_in(&process_directory)?;
    let input = directory.path().join("input");
    let output = directory.path().join("output");
    let data = directory.path().join("data");
    let temp = data.join("temp");
    for path in [&input, &output, &data, &temp] {
        fs::create_dir(path)?;
    }
    let input_path = input.join("episode.mkv");
    fs::write(&input_path, b"relative-root")?;
    let relative_input = input.strip_prefix(&process_directory)?.to_path_buf();
    let relative_output = output.strip_prefix(&process_directory)?.to_path_buf();
    let relative_data = data.strip_prefix(&process_directory)?.to_path_buf();
    let relative_temp = temp.strip_prefix(&process_directory)?.to_path_buf();

    // When: capabilities open the relative roots and an absolute task path.
    let capabilities = PathCapabilities::open(&PathConfig {
        input_roots: vec![relative_input],
        output_roots: vec![relative_output],
        data_root: relative_data,
        temp_root: relative_temp,
    })?;
    let rooted = capabilities.open_input(input_path)?;

    // Then: the task path is confined below the same descriptor-backed root.
    assert_eq!(rooted.snapshot().length, 13);
    Ok(())
}

#[test]
fn root_with_a_symlinked_ancestor_is_rejected() -> TestResult {
    // Given: a configured root reached through a symlinked ancestor directory.
    let directory = TempDir::new()?;
    let actual = directory.path().join("actual");
    fs::create_dir_all(actual.join("nested"))?;
    let linked = directory.path().join("linked");
    create_dir_symlink(&actual, &linked)?;

    // When: the symlinked path is opened as an input root.
    let result = PathCapabilities::open(&PathConfig {
        input_roots: vec![linked.join("nested")],
        output_roots: vec![actual.clone()],
        data_root: actual.clone(),
        temp_root: actual,
    });

    // Then: ambient traversal is rejected before a root capability is retained.
    assert!(matches!(result, Err(PathError::SymlinkComponent { .. })));
    Ok(())
}

#[test]
fn replacing_a_configured_root_invalidates_accepted_paths() -> TestResult {
    // Given: accepted input and output paths backed by the original configured roots.
    let fixture = Fixture::new()?;
    let input_path = fixture.input.join("episode.mkv");
    fs::write(&input_path, b"accepted-input")?;
    let input = fixture.capabilities.open_input(&input_path)?;
    let output = fixture
        .capabilities
        .open_output(fixture.output.join("episode.mp4"))?;

    // When: both configured root pathnames are replaced with different directories.
    fs::rename(&fixture.input, fixture.outside.join("old-input"))?;
    fs::rename(&fixture.output, fixture.outside.join("old-output"))?;
    fs::create_dir(&fixture.input)?;
    fs::create_dir(&fixture.output)?;

    // Then: queued work rejects both stale capabilities before reading or publishing.
    assert!(matches!(
        input.reopen_checked(),
        Err(PathError::RootChanged { .. })
    ));
    assert!(matches!(
        output.create_new(),
        Err(PathError::RootChanged { .. })
    ));
    assert!(!fixture.output.join("episode.mp4").exists());
    Ok(())
}

#[test]
fn rooted_input_reopens_only_the_snapshotted_regular_file() -> TestResult {
    // Given: a regular input opened through its configured root capability.
    let fixture = Fixture::new()?;
    let path = fixture.input.join("episode.mkv");
    fs::write(&path, b"original")?;
    let input = fixture.capabilities.open_input(&path)?;

    // When: the pathname is swapped to a symlink after the snapshot.
    let original = fixture.input.join("moved.mkv");
    fs::rename(&path, &original)?;
    create_file_symlink(&fixture.outside.join("secret.mkv"), &path)?;
    fs::write(fixture.outside.join("secret.mkv"), b"outside")?;

    // Then: reopening rejects the swap rather than reading outside the root.
    assert!(matches!(
        input.reopen_checked(),
        Err(PathError::SymlinkComponent { .. } | PathError::InputChanged { .. })
    ));
    Ok(())
}

#[test]
fn input_rejects_traversal_symlink_components_and_non_files() -> TestResult {
    // Given: an outside file, a symlinked directory, and a real directory below the input root.
    let fixture = Fixture::new()?;
    fs::write(fixture.outside.join("secret.mkv"), b"outside")?;
    create_dir_symlink(&fixture.outside, &fixture.input.join("linked"))?;
    fs::create_dir(fixture.input.join("folder"))?;

    // When: each invalid path is mapped as a local input.
    let traversal = fixture
        .capabilities
        .open_input(fixture.input.join("../outside/secret.mkv"));
    let symlink = fixture
        .capabilities
        .open_input(fixture.input.join("linked/secret.mkv"));
    let directory = fixture
        .capabilities
        .open_input(fixture.input.join("folder"));
    let windows = fixture.capabilities.open_input("C:\\outside\\secret.mkv");

    // Then: no invalid spelling or object reaches a readable file handle.
    assert!(matches!(traversal, Err(PathError::InvalidPath { .. })));
    assert!(matches!(symlink, Err(PathError::SymlinkComponent { .. })));
    assert!(matches!(directory, Err(PathError::InputNotRegular { .. })));
    assert!(matches!(windows, Err(PathError::InvalidPath { .. })));
    Ok(())
}

#[test]
fn valid_input_preserves_identity_size_and_mtime_on_reopen() -> TestResult {
    // Given: a stable regular file below an input root.
    let fixture = Fixture::new()?;
    let path = fixture.input.join("episode.mkv");
    fs::write(&path, b"stable-input")?;
    let input = fixture.capabilities.open_input(&path)?;

    // When: it is reopened immediately before transfer.
    let mut file = input.reopen_checked()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;

    // Then: the same snapshotted bytes are available through the capability handle.
    assert_eq!(bytes, b"stable-input");
    assert_eq!(input.snapshot().length, bytes.len() as u64);
    Ok(())
}

#[test]
fn output_creation_is_rooted_and_never_clobbers() -> TestResult {
    // Given: a non-existing output leaf below a configured output root.
    let fixture = Fixture::new()?;
    let path = fixture.output.join("episode.mp4");
    let output = fixture.capabilities.open_output(&path)?;

    // When: publication creates the leaf and a second creator races it.
    let mut file = output.create_new()?;
    file.write_all(b"published")?;
    let collision = output.create_new();

    // Then: the first bytes remain intact and the racing creator receives a collision.
    assert!(matches!(collision, Err(PathError::OutputExists { .. })));
    assert_eq!(fs::read(path)?, b"published");
    Ok(())
}

#[test]
fn output_can_create_missing_directories_below_the_nearest_existing_parent() -> TestResult {
    // Given: an output whose nested parent directories do not exist yet.
    let fixture = Fixture::new()?;
    let path = fixture.output.join("season/new/episode.mp4");
    let output = fixture.capabilities.open_output(&path)?;

    // When: the rooted output is created.
    output.create_new()?.write_all(b"published")?;

    // Then: only directories below the retained output capability are created.
    assert_eq!(fs::read(path)?, b"published");
    Ok(())
}

#[test]
fn output_parent_symlink_swap_cannot_escape_root() -> TestResult {
    // Given: an output capability whose parent existed during validation.
    let fixture = Fixture::new()?;
    let parent = fixture.output.join("season");
    fs::create_dir(&parent)?;
    let output = fixture
        .capabilities
        .open_output(parent.join("episode.mp4"))?;

    // When: the parent is replaced by a symlink to an outside directory.
    fs::rename(&parent, fixture.output.join("moved-season"))?;
    create_dir_symlink(&fixture.outside, &parent)?;
    let result = output.create_new();

    // Then: publication fails closed and creates nothing outside the root.
    assert!(matches!(
        result,
        Err(PathError::SymlinkComponent { .. } | PathError::OutputParentChanged { .. })
    ));
    assert!(!fixture.outside.join("episode.mp4").exists());
    Ok(())
}

#[cfg(unix)]
fn create_file_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_file_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
fn create_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}
