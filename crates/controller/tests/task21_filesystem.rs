#![expect(
    dead_code,
    reason = "the Task 13 publication fixture includes cleanup support not used by every adversarial case"
)]

#[path = "task13/checkpoints.rs"]
mod checkpoints;
#[path = "support/mock_videnoa/mod.rs"]
mod mock_videnoa;
#[path = "path_capabilities.rs"]
mod path_capabilities;
#[path = "task13/publication.rs"]
mod publication;
#[path = "task21/publication_race.rs"]
mod publication_race;
#[path = "task13/support.rs"]
mod support;
#[path = "task12/support.rs"]
mod transfer_support;
