# Videnoa Controller Task 5 Knowledge

## Durable Submission Boundary

- `POST /api/run` accepts an optional `Idempotency-Key`; existing unkeyed clients retain the original `201` response and independent job creation.
- Keys are 1 to 255 visible ASCII bytes. Empty, duplicate-header, non-UTF-8, whitespace-containing, and oversized values return `400` with `invalid_idempotency_key`.
- The durable mapping lives in the worker `jobs.db`, not in runtime maps. `idempotency_key` and `request_fingerprint` are nullable so legacy and unkeyed jobs remain valid.

## Fingerprint Contract

- The SHA-256 input contains the exact workflow name and params value.
- JSON objects are recursively serialized with keys sorted by UTF-8 bytes.
- Arrays preserve order; strings, numbers, booleans, and null preserve JSON scalar distinctions.
- Request formatting and object insertion order do not affect the fingerprint.
- JSON floating numbers use their shortest numeric spelling, with zero normalized
  to `0`, so equivalent values such as `1` and `1.0` share one fingerprint.
- If a stored hash predates numeric normalization, replay recomputes the current
  fingerprint from the persisted workflow name and params snapshot. This preserves
  durable mappings without consulting the current workflow file or accepting a
  genuinely changed request.

## Atomic Creator Election

- `JobsPersistence::claim_idempotent_job` uses a SQLite `BEGIN IMMEDIATE` transaction.
- The partial unique index `idx_jobs_idempotency_key` applies only to non-null keys.
- A new mapping is committed before runtime state insertion and `tokio::spawn`.
- Only the transaction winner calls `spawn_job`; replays return persisted ID, status, and creation time without creating a progress sender or executor.
- `lookup_idempotent_job` performs a persisted preflight immediately after
  fingerprinting. Replay and conflict return before current workflow file access.
- A preflight `Missing` result is advisory only. The final transactional claim
  remains authoritative because another request can insert between preflight and claim.

## Restart And Migration

- Startup keeps the existing behavior that reconciles queued/running jobs to cancelled.
- A replay after restart returns the same UUID and current cancelled status.
- Migration adds nullable columns atomically, rejects half-populated mappings, and refuses duplicate existing keys without deleting source rows.
- Legacy rows receive no synthetic key or fingerprint.

## Ambiguity Rule

- Durable idempotency requires persistent worker `data/`, specifically `jobs.db`.
- Database loss removes the evidence needed to distinguish an accepted request from an unaccepted request.
- Controller recovery must report `remote_state_ambiguous` and must not blindly resubmit.

## Evidence

- Happy, concurrency, restart, database, and regression proof: `.omo/evidence/videnoa-controller/task-5/idempotent-run.txt`.
- Conflict, malformed-key, migration-failure, and ambiguity proof: `.omo/evidence/videnoa-controller/task-5/idempotency-failures.txt`.
