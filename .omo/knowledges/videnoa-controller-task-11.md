# Videnoa Controller Task 11

Task 11 adds a durable worker registry and deterministic scheduler while keeping SQLite authoritative.

- Worker names are trimmed for persistence and case-insensitively unique; API URLs remain uniquely normalized.
- Worker policy and health use optimistic concurrency with typed duplicate, stale, referenced, and capacity errors.
- Capacity is derived from durable nonterminal task assignments; disabling a worker preserves existing ownership and blocks new claims.
- Scheduler task order is `priority DESC, created_at ASC, id ASC`; eligible worker order is `used_slots ASC, last_assigned_at ASC, id ASC`.
- Eligibility requires enabled, online, compatible workflow capability, available compute capacity, persisted unpaused settings, and the configured per-worker prefetch bound.
- `LifecycleService::reserve` remains the only assignment seam, with all eligibility predicates repeated in the atomic SQLite claim to close selection-to-write races.
- Upload candidates prioritize an idle feed before optional prefetch. Upload/download concurrency pools are independent, with one active upload permit per worker.
- Pause is persisted in controller settings and blocks reservation, upload, and submission while allowing poll, download, verify, publish, cleanup, and cancellation convergence.
- Worker policy updates atomically compare the requested compute slots with current nonterminal assignments in SQLite; stale versions and capacity rejection remain distinct typed outcomes.

Verification:

```bash
cargo fmt --all -- --check
cargo clippy -p videnoa-controller --all-targets --all-features -- -D warnings
cargo test -p videnoa-controller --test task11 -- --test-threads=1
cargo test -p videnoa-controller -- --test-threads=1
```
