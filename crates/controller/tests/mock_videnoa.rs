#[path = "support/mock_videnoa/mod.rs"]
mod mock_videnoa;

#[path = "mock_videnoa/faults.rs"]
mod faults;
#[path = "mock_videnoa/happy.rs"]
mod happy;
#[path = "mock_videnoa/idempotency.rs"]
mod idempotency;
#[path = "mock_videnoa/recovery.rs"]
mod recovery;
#[path = "mock_videnoa/recovery_contracts.rs"]
mod recovery_contracts;
#[path = "mock_videnoa/recovery_support.rs"]
mod recovery_support;
#[path = "mock_videnoa/restart.rs"]
mod restart;
#[path = "mock_videnoa/shutdown.rs"]
mod shutdown;
