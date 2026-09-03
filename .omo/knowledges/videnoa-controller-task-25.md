# Videnoa Controller Task 25

## Documentation Truth

- Keep archive recovery self-contained in `README-controller.md`. The archive
  has no `docs/` directory, so repository links may add depth but cannot carry
  first-run, backup/restore, ambiguity, or rollback requirements.
- Controller persistence is `data_root/controller.sqlite3` in SQLite WAL mode.
  A stopped-service filesystem backup copies the whole data root so any `-wal`
  and `-shm` files remain paired with the database.
- Worker persistence is a separate safety boundary. Videnoa `jobs.db` plus its
  task workspace is required to prove keyed submission and cleanup state. Loss
  means `remote_state_ambiguous`, never permission to submit again.
- SSE starts and falls back to `refetch`; its 64-entry broadcast is bounded and
  session auth is passively rechecked every 30 seconds. Task and attempt history
  stays server-paginated over HTTP and is never replayed through SSE.
- Documentation examples keep raw passwords out of command arguments by using
  hidden input and a mode-0600 temporary curl header file. TOML contains only a
  hash-file path, never password, PHC, Bearer, cookie, or CSRF material.

## Validation Pattern

- `scripts/tests/controller_docs_test.sh` checks required operator topics, exact
  routes, exact distribution names, task fields, config sections, repository
  links, stale product names, and plaintext secret assignments.
- `crates/controller/tests/controller_docs.rs` rewrites only example paths into
  an isolated temporary tree, generates an ephemeral Argon2id hash without
  recording it, and loads the real example through `ControllerConfig`.
- Native Windows packaging and hosted GitHub/Docker Hub publication remain
  platform boundaries. Linux validation can check exact workflow/script names
  and archive contracts without claiming those hosted operations ran locally.
