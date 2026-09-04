#![expect(
    dead_code,
    reason = "the shared wire-level mock exposes scenarios used by other integration targets"
)]

#[path = "support/mock_videnoa/mod.rs"]
mod mock_videnoa;

#[path = "task12/concurrency.rs"]
mod concurrency;
#[path = "task12/download.rs"]
mod download;
#[path = "task12/download_recovery.rs"]
mod download_recovery;
#[path = "task12/recovery_dispatch.rs"]
mod recovery_dispatch;
#[path = "task12/support.rs"]
mod support;
#[path = "task12/temp_security.rs"]
mod temp_security;
#[path = "task12/upload.rs"]
mod upload;
