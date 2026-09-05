use std::io::Write;

use tempfile::TempDir;

use crate::config::PathConfig;
use crate::domain::TaskId;

use super::PathCapabilities;

struct Fixture {
    _directory: TempDir,
    root: std::path::PathBuf,
    capabilities: PathCapabilities,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = TempDir::new()?;
        let root = directory.path().join("output");
        let data_root = directory.path().join("data");
        let temp_root = directory.path().join("temp");
        for path in [&root, &data_root, &temp_root] {
            std::fs::create_dir(path)?;
        }
        let capabilities = PathCapabilities::open(&PathConfig {
            input_roots: vec![root.clone()],
            output_roots: vec![root.clone()],
            data_root,
            temp_root,
        })?;
        Ok(Self {
            _directory: directory,
            root,
            capabilities,
        })
    }

    fn source(&self) -> Result<super::TempArtifact, Box<dyn std::error::Error>> {
        let workspace = self
            .capabilities
            .temp_workspace(TaskId::random(), true)?
            .ok_or_else(|| std::io::Error::other("temporary workspace was not created"))?;
        let source = workspace.artifact("output.bin.verified")?;
        source.create_truncated()?.write_all(b"durable bytes")?;
        Ok(source)
    }
}

#[test]
fn destination_parent_replacement_cannot_redirect_publication(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let parent = fixture.root.join("created");
    let output = fixture
        .capabilities
        .reopen_output(parent.join("final.bin"))?;
    let source = fixture.source()?;
    let finalizer = output.prepare_publication(&source)?;
    std::fs::rename(&parent, fixture.root.join("original"))?;
    std::fs::create_dir(&parent)?;

    finalizer.rename_noreplace()?;

    assert!(!parent.join("final.bin").exists());
    assert_eq!(
        std::fs::read(fixture.root.join("original/final.bin"))?,
        b"durable bytes"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn durability_sync_uses_retained_source_and_destination_directories(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let parent = fixture.root.join("parent");
    std::fs::create_dir(&parent)?;
    let output = fixture
        .capabilities
        .reopen_output(parent.join("final.bin"))?;
    let source = fixture.source()?;
    let finalizer = output.prepare_publication(&source)?;
    finalizer.rename_noreplace()?;
    std::fs::rename(&parent, fixture.root.join("original-parent"))?;
    std::os::unix::fs::symlink("missing-parent", &parent)?;

    finalizer.sync_parents()?;

    assert_eq!(
        std::fs::read(fixture.root.join("original-parent/final.bin"))?,
        b"durable bytes"
    );
    Ok(())
}

#[test]
fn external_output_publication_has_only_complete_final_entry(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let media = TempDir::new()?;
    let destination = media.path().join("E08.AI.mp4");
    let output = fixture.capabilities.open_output(&destination)?;
    let source = fixture.source()?;
    let finalizer = output.prepare_publication(&source)?;
    assert_eq!(std::fs::read_dir(media.path())?.count(), 0);
    finalizer.rename_noreplace()?;
    finalizer.sync_parents()?;
    assert_eq!(std::fs::read(&destination)?, b"durable bytes");
    assert_eq!(std::fs::read_dir(media.path())?.count(), 1);
    // A competing final destination is never replaced, even after preparation.
    let second = fixture.source()?;
    assert!(fixture
        .capabilities
        .reopen_output(&destination)?
        .prepare_publication(&second)?
        .rename_noreplace()
        .is_err());
    assert_eq!(std::fs::read(&destination)?, b"durable bytes");
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn cross_filesystem_rename_is_typed_and_leaves_media_empty(
) -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let media = TempDir::new_in("/dev/shm")?;
    let destination = media.path().join("E08.AI.mp4");
    // Recovery can reach finalization even if a mount relationship changed since intake.
    let output = fixture.capabilities.reopen_output(&destination)?;
    let source = fixture.source()?;
    let finalizer = output.prepare_publication(&source)?;
    assert!(
        matches!(finalizer.rename_noreplace(), Err(super::PathError::CrossFilesystemPublication { destination: actual, .. }) if actual == destination)
    );
    assert_eq!(std::fs::read_dir(media.path())?.count(), 0);
    Ok(())
}
