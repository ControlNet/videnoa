#![expect(
    dead_code,
    reason = "Task 20 compiles the complete shared fault harness across focused scenarios"
)]

#[path = "support/mock_videnoa/mod.rs"]
mod mock_videnoa;

#[path = "task20/cancellation.rs"]
mod cancellation;
#[path = "task20/cancellation_downstream.rs"]
mod cancellation_downstream;
#[path = "task20/fault_matrix.rs"]
mod fault_matrix;
#[path = "task20/fault_matrix_local.rs"]
mod fault_matrix_local;
#[path = "task20/fault_matrix_upload.rs"]
mod fault_matrix_upload;
#[path = "task20/multi_worker.rs"]
mod multi_worker;
#[path = "task20/one_worker.rs"]
mod one_worker;
#[path = "task20/outage_matrix.rs"]
mod outage_matrix;
#[path = "task20/pause.rs"]
mod pause;
#[path = "task20/remote_isolation.rs"]
mod remote_isolation;
#[path = "task20/retry.rs"]
mod retry;
#[path = "task20/shutdown.rs"]
mod shutdown;
#[path = "task20/support/mod.rs"]
mod support;
#[path = "task20/worker_health.rs"]
mod worker_health;
