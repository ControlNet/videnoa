# Single-request batch creation feasibility

## Inspected behavior

- `tasks/batch.rs` provides glob expansion, naming, per-row validation, duplicate-output detection, and a 500-file match limit. Preview only reads metadata.
- `tasks/intake.rs` validates each task, opens and verifies input/output capabilities, and calls atomic per-task persistence. Input checks read full file content through `paths/input_identity.rs`; `reopen_checked` verifies content again.
- `persistence/idempotency.rs` commits each task and its idempotency record in one transaction. It does not provide a batch transaction.
- `BatchTaskDialog.tsx` currently creates selected tasks one by one and stops on failure, retaining prior successes.

## Original proposal (implemented below with scope adjustments)

- Add `POST /api/tasks/batch` accepting the existing batch-preview options. Default new API submissions to `source: api` and `source_reference: null`.
- Reuse existing matching/naming and preflight validation, then perform task creation on the server and return task IDs plus explicit per-item outcomes/counts. Keep authentication identical to task creation and idempotency optional.
- Prefer explicitly documented per-task commits for the first version. An all-or-nothing contract requires preparing all inputs before a single batch database transaction and publishing events only after commit; filesystem validity cannot be guaranteed indefinitely by a database transaction.
- Robust explicit-key retries must persist the original expanded file list and batch progress. Re-expanding a glob on retry or using only row indices can change task identity after filesystem changes. This needs additional durable batch state, likely a migration.
- Full input hashing can make synchronous intake slow for large videos/batches. Move blocking preparation off the async executor; consider durable asynchronous intake with 202 and a status endpoint if long request latency must be avoided.
- Feasibility assessment only: no endpoint, migration, or live task was created.

## Implemented synchronous endpoint

- User selected synchronous HTTP and a strict preview gate: any preview error creates zero tasks.
- `POST /api/tasks/batch` accepts the existing preview options; it sets `source: api` and `source_reference: null` for creation.
- Full preview completes before any task creation. Invalid options, no matches, scan limits, or any invalid row return 400. Per-row rejection returns `created: 0`, `failed`, and `items` with requests/errors and null tasks; other validation errors retain the ordinary error envelope.
- Valid batches create sequentially and return 201 when all succeed, or 207 if creation fails after preview. Per-row `task`/`error` values identify successful and failed tasks. Successes are retained, and all rows are attempted. Response counts are `created` and `failed`.
- No async batch operation or migration was added. Input hashing moved into `spawn_blocking` through shared task preparation so synchronous HTTP intake does not block the async executor.
- Batch-level retry deduplication remains out of scope. The endpoint rejects supplied `Idempotency-Key` headers with 400 rather than silently ignoring them. Individually keyed task creation remains the deduplicated retry path.
- Six new HTTP regressions use existing test-only authentication/media fixtures and a test-only SQLite trigger for post-preview insertion failure. No real media was processed or deployed.
- Red-first run failed all five initial tests with 405 before route registration. Final targeted regression run passed 53 tests across task API, concurrency, persistence atomicity, paths, request contracts, auth HTTP, and documentation; explicit auth contention passed one additional test (54 total).
- Commands passed: `cargo +1.83.0 test --locked -p videnoa-controller --test task_api --test task_api_concurrency --test persistence_atomic --test path_capabilities --test task_contract --test controller_docs --test auth_http`; `cargo +1.83.0 test --locked -p videnoa-controller --test auth_contention -- --ignored --nocapture`; `cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings`; `cargo +1.83.0 fmt --all -- --check`; `rustfmt +1.83.0 --edition 2021 --check crates/controller/src/tasks/batch.rs`; `bash scripts/tests/controller_docs_test.sh`; `git diff --check`.
