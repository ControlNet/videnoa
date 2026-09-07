# Batch intake cost from source inspection

- Synchronous `TaskService::create_batch` first awaits metadata-only preview, then calls `create` sequentially for each row. It does not prepare or insert multiple tasks concurrently.
- Preview scans/matches names, computes output names, validates filesystem capabilities and detects collisions. Brace alternatives are matched in a single traversal rather than rescanning once per alternative.
- Each successful new task calls `prepare_task`: `open_input` calculates content identity by reading the entire file; the immediately following `reopen_checked` reads and hashes the entire file again to verify identity and contents.
- `paths/input_identity.rs` uses SHA-256 over 64 KiB reads, rewinds the file, and stores the first 16 digest bytes. This is full-content hashing, not sampled hashing; metadata is also checked before/after reads.
- Each task also performs an idempotency lookup, its own SQLite transaction inserting the task and ingress record, a task reload, and an SSE event publication.
- `spawn_blocking` keeps hashing off the async executor but is awaited per task; it does not introduce batch parallelism or shorten the synchronous response by itself.
- For total input size S, successful intake reads/hashes approximately 2*S logical bytes before returning, plus metadata/database overhead. OS/NAS caches may reduce physical disk reads but do not eliminate hashing work.
- This is a source-level diagnosis, not a measured performance profile of the user's deployment. Full-content verification and sequential preparation are the leading suspects for large-video batch latency; no performance patch or real-video test was made.

## Superseding correction (2026-09-07)

See [Controller input scans and transfer inactivity correction](controller-input-scans-transfer-inactivity-2026-09-07.md).
The historical behavior above is retained as a record. Duplicate intake/upload
hashes are removed, and upload uses an inactivity watchdog. Any advice above to
set transfer timeout beyond the complete upload duration is superseded; the
900-second default now bounds inactivity, not total transfer duration.
