# Videnoa Controller Task 13

## Publication admission

- `FinishVerification(PublicationIntent)` commits `Verifying -> Publishing` and binds the unique hidden destination staging name in the same paired task/attempt CAS.
- Expected output length and SHA-256 remain the durable evidence written by Task 12 when download enters `Verifying`; publication requires both values and the Task 12 verified artifact to agree before effects.
- Existing destinations detected before admission close with typed nonretryable `OutputExists` while preserving the destination and verified source.

## No-clobber publication

- Publication always copies the verified temp into a destination-owned `.videnoa-<task>-<uuid>.staging` file opened with `create_new`, so same-filesystem and cross-filesystem sources share one recovery model and never rename directly from Controller temp.
- Copying is bounded to 64 KiB, recomputes length/SHA-256, rejects growth beyond durable length, flushes, and syncs the staging file.
- Linux finalization uses safe `rustix::fs::renameat_with` with `RENAME_NOREPLACE` relative to the capability-opened destination parent. Windows retains `renamore::rename_exclusive` after immediate root/parent identity revalidation.
- Recovery accepts only an exact final or exact owned staging hash/length. Mismatch, unsafe staging identity, or contradictory ownership becomes nonretryable `PublicationAmbiguous` and preserves all files.
- Final and staging nodes must be regular files before opening. A matching final proves publication only when the durable staging name is absent.
- Identities for destination parents created during staging remain pinned until finalization, preventing replacement with a different directory namespace.

## Cleanup convergence

- `Publishing -> RemoteCleanup` commits before cleanup effects.
- Cleanup removes Controller-owned task temp first and then DELETEs the remote task workspace.
- DELETE success and 404 complete the task. Network, timeout, stall, and 5xx persist bounded paired cleanup retry metadata. Other 4xx and malformed/configuration outcomes close terminally with `CleanupFailed` at `RemoteCleanup`.
- Cleanup retries stay on the same attempt and never repeat compute or publication.

## Recovery and verification

- Startup recovery dispatch now executes `Verify`, `Publish`, and `Cleanup` commands through `TransferExecutor`.
- Non-cancelled verifying/publishing/cleanup stages are emitted before worker health checks, allowing local publication to converge while an offline worker becomes a durable remote-cleanup retry.
- The Task 13 real SQLite/filesystem/HTTP target has 14 deterministic cases covering existing destinations, matching final/staging recovery, contradictory and non-regular artifacts, DELETE 404/400/500, offline local-first recovery, malformed cleanup isolation, and production startup dispatch.
- A direct path-capability unit regression replaces a parent created during staging and proves finalization rejects the changed identity while preserving both verified and unrelated bytes.
- The focused suite does not claim forced EXDEV, permission denial, FIFO, cancellation integration, or every requested crash window as executed coverage.
- Strict formatting, Clippy, the full Controller suite, Controller build, diagnostics, and Rust no-excuse checks pass.
