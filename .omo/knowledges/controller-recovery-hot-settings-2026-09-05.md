# Controller Recovery Hot Settings

- `RecoveryConfig` is the recovery-owned projection boundary for timeout and retry settings.
- Attach `scheduler.runtime_settings().clone()` with `RecoveryConfig::with_runtime_settings(...)`; clones share the scheduler's `Arc<RwLock<_>>` and observe later `reconfigure` calls.
- `Reconciler::reconcile_task` resolves `remote_timeouts()` when constructing each new `VidenoaClient`.
- `Reconciler::defer_worker` resolves `health_retry()` for each failed worker-health recovery operation.
- Production startup currently has this wiring in `main.rs`; any full-runtime test fixture constructing `RecoveryConfig` must attach the same shared handle or it will intentionally retain constructor fallbacks.
- Focused regressions live in `crates/controller/src/recovery/model.rs` and use distinct startup, initial runtime, and updated runtime values so stale selection cannot pass accidentally.
