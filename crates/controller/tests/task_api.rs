// Shared with task_api_concurrency; each target uses a different subset of helpers.
#[allow(dead_code)]
#[path = "task_api/support.rs"]
mod support;

#[path = "task_api/authentication.rs"]
mod authentication;
#[path = "task_api/batch.rs"]
mod batch;
#[path = "task_api/batch_create.rs"]
mod batch_create;
#[path = "task_api/batch_wildcards.rs"]
mod batch_wildcards;
#[path = "task_api/intake_contract.rs"]
mod intake_contract;
#[path = "task_api/path_suggestions.rs"]
mod path_suggestions;

#[path = "task_api/batch_idempotency.rs"]
mod batch_idempotency;
