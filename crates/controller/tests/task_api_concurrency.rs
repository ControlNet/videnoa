// Keep the 100 ms, single-connection SQLite fixture isolated from unrelated
// parallel fixture migrations. This ordinary regression is never ignored.
#[allow(dead_code)]
#[path = "task_api/support.rs"]
mod support;

#[path = "task_api/concurrency.rs"]
mod concurrency;
