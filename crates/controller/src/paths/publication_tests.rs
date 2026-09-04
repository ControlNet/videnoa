use std::io::Write;
#[cfg(target_os = "linux")]
use std::sync::mpsc;
#[cfg(target_os = "linux")]
use std::time::Duration;

use tempfile::TempDir;

use crate::config::PathConfig;

use super::{PathCapabilities, PathError, PublicationArtifact};

#[test]
fn created_publication_parent_replacement_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let root = directory.path().join("output");
    let data_root = directory.path().join("data");
    let temp_root = directory.path().join("temp");
    for path in [&root, &data_root, &temp_root] {
        std::fs::create_dir(path)?;
    }
    let config = PathConfig {
        input_roots: vec![root.clone()],
        output_roots: vec![root.clone()],
        data_root,
        temp_root,
    };
    let capabilities = PathCapabilities::open(&config)?;
    let output = capabilities.reopen_output(root.join("created/final.bin"))?;
    output
        .create_staging(".videnoa-created-parent.staging")?
        .write_all(b"verified")?;
    std::fs::rename(root.join("created"), root.join("original"))?;
    std::fs::create_dir(root.join("created"))?;
    std::fs::write(
        root.join("created/.videnoa-created-parent.staging"),
        b"unverified",
    )?;

    let result = output
        .prepare_finalization(".videnoa-created-parent.staging")
        .and_then(|finalizer| finalizer.rename_noreplace());

    assert!(matches!(result, Err(PathError::OutputParentChanged { .. })));
    assert!(!root.join("created/final.bin").exists());
    assert_eq!(
        std::fs::read(root.join("created/.videnoa-created-parent.staging"))?,
        b"unverified"
    );
    assert_eq!(
        std::fs::read(root.join("original/.videnoa-created-parent.staging"))?,
        b"verified"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn parent_sync_uses_the_directory_opened_for_finalization() -> Result<(), Box<dyn std::error::Error>>
{
    // Given: a finalizer has renamed staging through a retained parent descriptor.
    let directory = TempDir::new()?;
    let root = directory.path().join("output");
    let parent = root.join("parent");
    let data_root = directory.path().join("data");
    let temp_root = directory.path().join("temp");
    std::fs::create_dir_all(&parent)?;
    std::fs::create_dir(&data_root)?;
    std::fs::create_dir(&temp_root)?;
    let capabilities = PathCapabilities::open(&PathConfig {
        input_roots: vec![root.clone()],
        output_roots: vec![root.clone()],
        data_root,
        temp_root,
    })?;
    let output = capabilities.reopen_output(parent.join("final.bin"))?;
    output
        .create_staging(".videnoa-parent-sync.staging")?
        .write_all(b"durable bytes")?;
    let finalizer = output.prepare_finalization(".videnoa-parent-sync.staging")?;
    finalizer.rename_noreplace()?;
    std::fs::rename(&parent, root.join("original-parent"))?;
    std::os::unix::fs::symlink("missing-parent", &parent)?;

    // When: durability sync runs after the ambient parent has been replaced.
    finalizer.sync_parent()?;

    // Then: sync succeeds on the retained descriptor and the renamed bytes remain in that directory.
    assert_eq!(
        std::fs::read(root.join("original-parent/final.bin"))?,
        b"durable bytes"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn regular_staging_swapped_to_fifo_is_classified_from_the_open_handle(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given: a regular staging leaf is swapped to a FIFO immediately before its production open.
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
    let output = capabilities.reopen_output(root.join("final.bin"))?;
    let staging_name = ".videnoa-fifo-swap.staging";
    output.create_staging(staging_name)?.write_all(b"regular")?;
    let staging = root.join(staging_name);
    let writer_path = staging.clone();
    let (sender, receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = output.open_staging_with_checkpoint(staging_name, || {
            std::fs::remove_file(&staging)?;
            let status = std::process::Command::new("mkfifo")
                .arg(&staging)
                .status()?;
            if !status.success() {
                return Err(std::io::Error::other("mkfifo failed"));
            }
            Ok(())
        });
        let _ignored = sender.send(result);
    });

    // When: the opened-handle classification executes without waiting for a FIFO writer.
    let result = match receiver.recv_timeout(Duration::from_millis(250)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _writer = std::fs::OpenOptions::new().write(true).open(&writer_path)?;
            receiver.recv_timeout(Duration::from_secs(1))?
        }
        Err(error) => return Err(Box::new(error)),
    };
    worker
        .join()
        .map_err(|_| std::io::Error::other("FIFO swap worker panicked"))?;

    // Then: the FIFO is returned as non-regular rather than a readable publication artifact.
    assert!(matches!(result?, PublicationArtifact::NonRegular));
    Ok(())
}
