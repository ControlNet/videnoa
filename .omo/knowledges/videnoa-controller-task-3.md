# Videnoa Controller Task 3

- Controller state uses SQLx 0.8.6 with SQLite WAL, foreign keys, a five-second busy timeout, and a bounded pool.
- The production migration owns six tables: tasks, task_attempts, workers, controller_settings, auth_sessions, and task_idempotency.
- Persist enum values as stable snake_case strings and UTC timestamps as integer milliseconds.
- Reservation is one transaction: conditionally update a queued task only for an enabled, online worker with free capacity, then insert its attempt.
- Task ingress inserts the task and idempotency mapping atomically; duplicate matching fingerprints replay the original task, while mismatches conflict and rollback the candidate task.
- Repository pagination is SQL-bounded and deterministic. EXPLAIN evidence over 20,000 rows confirms planned indexes for filters, sorts, queue selection, and startup recovery.
- Session rows store only token, CSRF, and password-hash digests. No plaintext credentials are persisted.
- Rust 1.83 verification is currently blocked before compilation by ignored lockfile resolution selecting base64ct 1.8.3, whose manifest requires Cargo edition-2024 support.
