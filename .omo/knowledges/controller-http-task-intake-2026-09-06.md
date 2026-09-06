# Controller HTTP task intake

- Source checked: `docs/controller.md`, `README-controller.md`, `crates/controller/src/domain/task.rs`, and `crates/controller/src/tasks/routes.rs`.
- After initial setup, API clients can authenticate with the administrator password as a Bearer credential; no cookie login, Origin, or CSRF header is required for Bearer requests.
- `POST /api/tasks` accepts `input_path`, `output_path`, `workflow`, integer `priority` (-100 through 100), `source` (`api` or `manual`), and optional nullable `source_reference`. Unknown fields are rejected.
- `Idempotency-Key` is now optional. Missing headers generate a fresh internal UUID per request, using existing atomic persistence without a migration. Repeated unkeyed requests can create independent tasks.
- If supplied, a single `Idempotency-Key` header must contain 1 through 255 visible ASCII bytes. Keep the same key and body for retries: initial creation returns 201, replay returns 200, changed body with the same key returns 409. Explicit empty, invalid, and repeated headers still return 400.
- The response is the task object directly, including `id` and `status`. `GET /api/tasks/{id}` returns a wrapper with `task`, `attempts`, and pagination metadata.
- Paths refer to the Controller process filesystem, including container mount paths in Docker. Input must exist as a regular file; output must not already exist; private storage is excluded.
- Actual execution needs an enabled, reachable worker with the named compatible workflow/preset and Path inputs named `input` and `output`, plus an unpaused scheduler.
- Interactive curl examples should read passwords without echo and pass Authorization through stdin (`--header @-`) rather than expanding the password into curl arguments. Do not persist credentials in source or logs.
- No live task was submitted during this documentation check.
- Batch HTTP intake can follow the UI's two-step flow: `POST /api/tasks/batch-preview` returns candidate `items[].request` bodies and per-row errors without creating tasks; submit each selected valid body through `POST /api/tasks`. Preview emits `source: manual`; external API callers should set `source: api` in the subsequent creation bodies when they want API provenance.
- A synchronous `POST /api/tasks/batch` now accepts preview options and automatically creates API-sourced tasks after a clean full preview. Any preview failure rejects all creation; post-preview creation failures return 207 with per-row results, retaining successes. Full success returns 201. No key is required; explicit batch keys are rejected because batch retry deduplication is not implemented. See `controller-batch-create-feasibility-2026-09-06.md` for implementation verification.
- `source` is required caller-provided provenance (`manual` or `api`), stored and shown in task details and supported as a list filter. `source_reference` is optional caller-provided external correlation metadata (up to 512 UTF-8 bytes), stored and shown in details. Neither chooses workers or changes processing. Both are part of the serialized request fingerprint for explicit-key replay comparison; `source_reference` is not a deduplication key.

## Implementation verification

- Red-first regression: unkeyed creation failed with 400 before the route change.
- `cargo +1.83.0 test --locked -p videnoa-controller --test task_api --test task_api_concurrency --test controller_docs`: 22 tests passed, including unkeyed independent creation, existing-output validation, invalid headers, Bearer/CSRF boundaries, and keyed concurrent replay.
- `cargo +1.83.0 fmt --all -- --check`: passed.
- `cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings`: passed.
- `bash scripts/tests/controller_docs_test.sh` and `git diff --check`: passed.
- Existing synthetic test media and test-only authentication fixtures are reused; no real media processing or deployment was performed.
