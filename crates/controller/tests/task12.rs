#![expect(
    dead_code,
    reason = "the shared wire-level mock exposes scenarios used by other integration targets"
)]

#[path = "support/mock_videnoa/mod.rs"]
mod mock_videnoa;

#[path = "task12/support.rs"]
mod support;
#[path = "task12/upload.rs"]
mod upload;
