// Shared with task_api_concurrency; each target uses a different subset of helpers.
#[allow(dead_code)]
#[path = "task_api/support.rs"]
mod support;

#[path = "task_api/authentication.rs"]
mod authentication;
#[path = "task_api/intake_contract.rs"]
mod intake_contract;
