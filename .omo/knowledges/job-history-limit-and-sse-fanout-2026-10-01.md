# Worker job history (kept unbounded) and Controller SSE fan-out (2026-10-01)

## Worker job history: no automatic limit (reverted 2026-10-02)

A `[jobs] history_limit` (default 1000) that pruned finished jobs was added in
`4898246` and reverted in `1a7d75b`. Do not reintroduce automatic pruning of
Worker jobs; it breaks the Controller-Worker contract:

- The Controller polls `GET /api/jobs/{id}` until a terminal state
  (`crates/controller/src/recovery/processing.rs`); a pruned job returns 404
  and the task fails as `remote_state_ambiguous`.
- The idempotency key lives on the job row, so pruning it turns a replayed
  `POST /api/run` into a duplicate run instead of `Replayed`.
- `processing_retry` (`operations/tasks.rs`) looks up the original remote job.
- The Controller never deletes Worker job records: `DELETE /api/jobs/{id}` is
  sent only on cancellation (`recovery/submission.rs`) and remote cleanup
  (`scheduler/cleanup_remote.rs`) removes workspace files only.

Finished jobs are small rows; users delete them manually via Job History
(`delete_job_history`). If bounding is ever needed, it must be driven or
acknowledged by the Controller, not by a Worker-local count.

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
