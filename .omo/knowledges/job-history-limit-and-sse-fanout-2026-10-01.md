# Worker job history limit and Controller SSE fan-out (2026-10-01)

## Worker: `[jobs] history_limit`

- `crates/core/src/config.rs` `JobsConfig { history_limit: usize }`, default
  `1000`; `0` keeps the full history (TOML cannot express "unset" once a
  default exists, and `0 = keep none` would make clients polling a finished job
  see 404 immediately).
- Logic lives in `crates/core/src/server/history.rs`:
  - finished = `JobStatus::is_terminal()` (completed, failed, cancelled);
    ordered by `completed_at`, falling back to `created_at`, then id.
  - Runtime: `AppState::enforce_job_history_limit()` runs at the end of
    `run_job`. Each pruned job holds its DashMap entry until
    `JobsPersistence::delete_job` succeeds (same ordering as
    `delete_job_history`); a failed delete keeps the job.
  - Startup: `history::prune_restored()` runs in `AppState::new` after
    `load_jobs_for_startup` (so Running/Queued rows are already reconciled to
    Cancelled and count as finished), deleting all overflow rows in one
    transaction (`JobsPersistence::delete_jobs`). On failure nothing is dropped.
- A lowered limit saved through `PUT /api/config` applies at the next job
  completion or restart, not immediately.
- Pruning removes the idempotency mapping stored on the row, exactly like a
  manual history delete.

## Controller: one store read per durable change

- `crates/controller/src/operations/events.rs`: `LiveEvent::DurableChange`
  carries `Arc<SharedChange { change, event: tokio::sync::OnceCell<Event> }>`.
  The first SSE subscriber that reaches the change runs `durable_change_event`
  inside `get_or_init`; every other subscriber clones the same `Event`.
  `OnceCell` is cancellation-safe: if that subscriber disconnects mid-read,
  another waiter takes over.
- No extra task or lifecycle: ordering with `Delta` events, initial `refetch`,
  `Lagged` -> `refetch`, 30 s passive re-auth, shutdown and listener
  `take_until` are unchanged. With zero subscribers there are zero reads.
- Behaviour change: all connections now see the same `event_id` for a durable
  change (previously random per connection).
- Test counter: `EventHub::durable_change_reads()`; tests in
  `crates/controller/tests/task14/sse.rs`.
