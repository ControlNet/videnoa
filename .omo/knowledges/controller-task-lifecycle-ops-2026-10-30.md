# Controller task lifecycle operations (2026-10-30)

These fixes come from design review findings J and K, and from controller #5
("the state vocabulary is copied in many places"). The review is in
`design-review-2026-10-30.md`.

## Operator actions and what they guarantee

- **Abandon** (`POST /api/tasks/{id}/abandon`)
  - It needs a requested cancellation, and the task must be in a state that waits
    for the Worker (`Lifecycle::abandonable`: uploading through verifying).
  - The task fails with `worker_unavailable` at `remote_cleanup`. The schema's
    CHECK lists cannot change without rebuilding the table, so the code reuses
    `worker_unavailable`, which existed but was never produced.
  - The handler commits first, then removes the local workspace on a best-effort
    basis. It never calls the Worker.
- **Worker delete**
  - Deletion is allowed once none of the Worker's tasks or attempts is
    unfinished. It used to require that the Worker had no history at all.
  - Inside one transaction it:
    1. copies the Worker name into `task_attempts.worker_name` (migration 0015);
    2. sets `worker_id` to NULL on attempts and tasks;
    3. deletes the Worker.
  - `ON DELETE RESTRICT` fires immediately, so the delete must come after the
    NULLs. `requested_worker_id` is `ON DELETE SET NULL`, which unpins queued
    retries; that is why migration 0014's "delete the Worker" advice now works.
- **Discard** (`POST /api/tasks/{id}/discard`, migration 0016 `discarded_at_ms`)
  - Only for failed tasks. It is final: retry and a second discard return 409.
    Three layers enforce this: the handler, the lifecycle, and the SQL
    `discarded_at_ms IS NULL` guard.
  - After the commit, the handler removes local temp files. It then deletes the
    Worker task folder only when the Worker is online and the job, if any, is
    terminal (checked with `terminal_job`).
  - The response field `worker_folder_deleted` reports which happened.
- **Automatic release of local temp** (`TransferExecutor::release_unneeded_workspace`)
  - It runs after every stage and in a startup sweep before any stage starts.
    The sweep is called from `recovery_scan.rs`.
  - `scheduler::local_files_unneeded` decides what is released:
    - completed, cancelled or discarded tasks;
    - failed tasks whose `Lifecycle::retry_mode` is `Blocked`.
  - Failures that can be resumed keep the verified output, because download,
    verify and publish retries reuse it.
  - The sweep only touches directories named by a known TaskId.

## History retention (`tasks/retention.rs`, `persistence/history_retention.rs`)

- An hourly loop runs as the fourth `try_join!` arm in `main.rs`.
- Batch `Idempotency-Key` receipts always expire after 7 days
  (`BATCH_RECEIPT_DAYS`).
- `[history] retention_days` in `controller.toml` is off by default.
  - When set, the loop purges only tasks that are both terminal and
    `local_files_unneeded`. A failure that could still be retried stays, because
    its attempt is the only record of the Worker folder.
  - Order:
    1. temp files go first, because the startup sweep keeps directories of
       unknown tasks;
    2. one transaction deletes `task_idempotency`, then the attempts, then the
       task, all guarded by version.
- A config section that only lives in the file needs four edits, or it is
  silently dropped:
  - `config/raw.rs`, with `skip_serializing_if`;
  - `config/validate.rs`;
  - `config/document.rs`: `From<&ControllerConfig>` is what the settings save
    writes back;
  - `operations/settings/validate.rs`, which must carry `current.<section>`.
- Tests that catch a missing edit:
  - `config_contract::history_retention_is_off_by_default_validated_and_preserved`;
  - `task14::settings::settings_updates_keep_history_retention_from_the_config_file`.

## One state vocabulary

- `snake_case_enum!` generates `ALL`.
- `persistence/status_sql.rs` builds the SQL `IN (...)` lists from `TaskStatus`
  predicates. The partial indexes need the exact text
  `NOT IN ('completed', 'failed', 'cancelled')`, and a test pins it.
- `Task.allowed_actions` is computed by `Lifecycle::allowed_actions`, using the
  same rules the handlers enforce.
  - Retry needs the task's `worker_id`. A deleted Worker leaves it NULL, and
    failed tasks otherwise keep it.
  - The web `taskActionPolicy.ts` only reads that list. The zod schema is
    `z.array(z.string())`, so actions the UI does not know are ignored.
  - The e2e fixture (`tests/e2e/tasks-fixtures.ts`) computes the list from each
    fixture's state so that existing suites keep their buttons.

## Released-database fixtures

`crates/controller/tests/fixtures/released_databases/v0.1.9.sql` and
`v0.1.10.sql` are `sqlite3 .dump` files. To regenerate them:

1. For each release tag, copy that tag's migrations into a directory, reading
   them with `git ls-tree` and `git show <tag>:crates/controller/migrations/<file>`.
2. Run them with `sqlx::migrate::Migrator` on an empty database.
3. Insert the seed rows. All rows exist in both releases, except a retried task
   that v0.1.10 pins to the retired Worker.
4. Dump the database with `sqlite3 <db> .dump`.

The generator was a throwaway test and is not committed. `persistence_upgrade.rs`
loads each dump with `sqlx::raw_sql`. `Database::open` then fails with
`VersionMismatch` if a released migration was edited. The same fixtures also
drive the retention and `allowed_actions` tests.

## Gotchas

- `cargo fmt` does not follow `include!` modules, so run
  `git ls-files -z '*.rs' | xargs -0 -n1 rustfmt --edition 2021 --check`.
  Running `rustfmt` on a `mod.rs` also formats its child modules, so check
  `git status` for files you did not mean to touch.
- `run_controller` in `main.rs` sits at clippy's 100-line limit. Pass whole
  config structs, such as `HistoryRetention::new(.., config.history)`, rather
  than adding lines.
- Task20 Worker deletes can race health probes that bump the Worker version.
  The `delete_worker_status` helper retries on a stale-version 409.
- `sse_shutdown_process` needs `controller-web/dist`. Run `npm run build` before
  the controller suite.
