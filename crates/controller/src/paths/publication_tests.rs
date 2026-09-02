use std::io::Write;

use tempfile::TempDir;

use crate::config::PathConfig;

use super::{PathCapabilities, PathError};

#[test]
fn created_publication_parent_replacement_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let root = directory.path().join("output");
    std::fs::create_dir(&root)?;
    let config = PathConfig {
        input_roots: vec![root.clone()],
        output_roots: vec![root.clone()],
        data_root: directory.path().join("data"),
        temp_root: directory.path().join("temp"),
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

    let result = output.finalize_staging(".videnoa-created-parent.staging");

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
