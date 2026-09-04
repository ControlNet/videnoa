mod actions;
mod admission;
#[path = "controller.rs"]
mod harness;
mod http;
mod proof;
mod recovery;
mod runtime;
#[path = "checkpoint.rs"]
mod synchronization;

pub use harness::{ControllerFixture, TestResult};
pub use proof::{
    assert_completed_pipeline, assert_restarted_pipeline, coherent_task_attempt, complete_mock_job,
    lifecycle_operation_error, wait_for_positive_download_partial,
};
pub use synchronization::CheckpointGate;
