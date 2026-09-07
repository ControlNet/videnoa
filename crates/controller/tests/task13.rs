#![expect(
    dead_code,
    reason = "the shared wire-level mock exposes scenarios used by other integration targets"
)]

#[path = "support/mock_videnoa/mod.rs"]
mod mock_videnoa;

#[path = "task13/cancellation.rs"]
mod cancellation;
#[path = "task13/checkpoints.rs"]
mod checkpoints;
#[path = "task13/cleanup.rs"]
mod cleanup;
#[path = "task13/publication.rs"]
mod publication;
#[path = "task13/publication_ambiguity.rs"]
mod publication_ambiguity;
#[cfg(target_os = "linux")]
#[path = "task13/publication_copy.rs"]
mod publication_copy;
#[path = "task13/publication_durability.rs"]
mod publication_durability;
#[path = "task13/publication_nonregular.rs"]
mod publication_nonregular;
#[path = "task13/recovery.rs"]
mod recovery;
#[path = "task13/support.rs"]
mod support;
#[path = "task13/temp_cleanup_security.rs"]
mod temp_cleanup_security;
#[path = "task12/support.rs"]
mod transfer_support;
