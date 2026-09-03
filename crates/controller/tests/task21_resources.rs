#![expect(
    dead_code,
    reason = "the Task 12 wire mock exposes routes beyond the Task 21 saturation scenario"
)]

#[path = "task12/concurrency.rs"]
mod concurrency;
#[path = "support/mock_videnoa/mod.rs"]
mod mock_videnoa;
#[path = "task12/support.rs"]
mod support;
