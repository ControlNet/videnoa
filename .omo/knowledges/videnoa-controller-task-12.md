# Videnoa Controller Task 12

## Durable transfer boundary

- `AdvanceCommand::FinishUpload` carries exact opaque input/output workflow paths. The paired task/attempt CAS stores them while entering `staged`.
- `AdvanceCommand::FinishDownload` carries exact byte length and SHA-256. The paired CAS stores verification evidence while entering `verifying`.
- Submission and cancellation reconciliation require their paths to equal the previously persisted upload evidence; recovery no longer re-derives workflow paths.
- Task and attempt retry metadata are written atomically for upload/download retries and reset together after a proven successful transfer.

## Upload stage

- The executor acquires the Task 11 per-worker/global upload permit before changing `reserved` to `uploading`.
- Input is reopened through `PathCapabilities`, with identity, size, and millisecond-normalized mtime checked against intake evidence.
- PUT always reconciles through exact stat. Exact length stages even after an uncertain response; mismatched owned targets are deleted and durably retried, while a due restart that proves the target absent immediately reopens the rooted input and PUTs from zero.
- File API paths remain task-owned (`<task>/input.<ext>`), while returned workflow paths remain opaque durable values.

## Download stage

- The independent download permit allows overlap with saturated uploads.
- A valid Controller-owned `.verified.evidence` plus matching `.verified` file is reconciled before worker lookup or network access. Otherwise the remote output is statted and must be a non-empty file; GET requires `Content-Length` and restarts into a freshly truncated `.part` file.
- `HashingWriter` computes SHA-256 and length as chunks are written. The file is flushed, synced, and renamed within the task temp directory to `output.<ext>.verified` before lifecycle evidence commits.
- The fixed 40-byte local evidence record stores big-endian length plus SHA-256 and is file- and directory-synced before the verified rename. Missing, malformed, zero-length, or hash-mismatching evidence removes Controller-owned temporary artifacts before a fresh GET.
- Failed or truncated bodies remove `.part`; restart never sends `Range` and never resubmits compute.

## Verification

- Strict Clippy, full Controller tests, and Controller build pass.
- The real-TCP Task 12 suite has 18 cases covering 20 KB streaming, exact opaque paths, mismatch cleanup, restart PUT-from-zero, cleanup failure durability, pause admission and startup deferral, input replacement, retry deadlines, malformed remote evidence, truncation cleanup, zero output, stale-part restart, verified-artifact crash boundaries, no Range, hash persistence, independent transfer checkpoints, and production recovery dispatch.

## Review fixes

- Retry resets bind task and attempt fields in SQL placeholder order, and download admission requires both durable retry deadlines to be due.
- Startup recovery routes transfer commands through the production `TransferExecutor`, then re-runs keyed reconciliation for tasks advanced by transfer recovery.
- The scheduler pause predicate is repeated inside the atomic `reserved -> uploading` write, closing the selection-to-admission race.
- Restart upload stat-checks before PUT; exact remote bytes stage immediately, while absent or mismatched bytes retry from zero. Cleanup failure is surfaced only after paired retry metadata commits.
- Missing or replaced local input closes durably with typed nonretryable failure codes instead of returning an unpersisted filesystem error.
- Download requires complete, mutually consistent remote job/input/output evidence before any network request.
- A verified artifact and its local length/hash evidence are validated before any remote request: matching bytes resume the lifecycle CAS after rename-before-CAS crashes, while missing or mismatching evidence is removed before a fresh GET. Parent-directory sync makes both evidence creation and verified rename durable on supported platforms.

## Convergence fixes

- `Uploading + stat NotFound` is not a retryable remote outage: it proves there is no prior remote target to reconcile, so the current due invocation executes the existing root-confined fresh upload path without another `StartUpload` transition.
- Startup upload deferral is classified only when durable state is exactly `scheduler.paused && task.status == Reserved`, checked both before dispatch and after a conflict to cover the pause race without swallowing unrelated lifecycle conflicts.
- Crash recovery needs independently trustworthy local evidence. File existence or non-zero length alone cannot distinguish a matching verified artifact from stale bytes, so the sidecar evidence is part of the Controller-owned temporary artifact protocol.

## Windows durability review

- Unix retains parent-directory `sync_all` after evidence creation, verified rename, and owned-artifact removal.
- Windows treats parent-directory synchronization as a successful no-op only because artifact and evidence files individually complete `sync_all` before same-directory rename.
- Platforms that are neither Unix nor Windows retain a typed `Unsupported` error instead of silently weakening durability.
- A pure platform-policy regression runs on Linux and locks the Windows contract without pretending to execute Windows filesystem syscalls.
- Native formatting, strict Clippy, Task 12 tests, and the full Controller suite pass. MSVC cross-checking requires an MSVC-compatible C compiler and archiver that are not installed on this Linux host.

## Delivery

- The original Task 12 delivery was split into 11 English semantic commits from `f1b96de` through `ad969ad`; review fixes were verified afterward.
- Every staged commit passed the secret scan. The full tracked scan reported only the deliberate `token=` redaction test literal in `crates/core/src/logging.rs`.
- `dev` was rebased, pushed, pruned, and confirmed synchronized with `origin/dev` at `ad969ad`.
