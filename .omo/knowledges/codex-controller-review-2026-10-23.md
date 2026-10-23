# Codex (gpt-6-astra) controller review (2026-10-23, at 58c9218)

This was a read-only review of `crates/controller` and `controller-web`. Codex
was given the worker contract and today's worker fixes from
`codex-project-review-2026-10-23.md`, and was asked to consult the earlier
`controller-*.md` design notes before reporting anything as a bug.

Claude has re-read the code for findings 1 and 2 and confirmed both.
Findings 3-5 are as reported by Codex and have not been re-checked.

## Findings

1. **High, confirmed: a stale retry can delete a restarted attempt's workspace.**
   - Where: `operations/tasks.rs` `requeue`.
   - The handler calls `client.delete_file({task_id})` on the worker before
     `retry_requeue` performs its CAS.
   - Two Retry requests at the same task version can interleave like this:
     1. Request A deletes the workspace, commits, and the task is
        rescheduled to the same worker.
     2. Request A's new attempt uploads its input.
     3. Request B's delayed DELETE then wipes the new attempt's
        `{task_id}/` directory. Request B's CAS fails afterwards, but the
        workspace is already gone.
   - Remote directories are named by task ID, not attempt ID.
   - Fix: hold a per-task exclusion around the whole retry, and re-validate
     the task version before any remote cleanup.
2. **High, confirmed: cancelling during a download or upload shuts the
   controller down.**
   - `request_cancellation` for a `Downloading` task only commits the
     cancellation intent; the status stays the same and the version bumps.
     The in-flight transfer is not aborted.
   - When the download finishes, `scheduler/download.rs:108` calls
     `advance(FinishDownload)` with the old snapshots. The CAS fails with
     `LifecycleError::Conflict`, which reaches the caller as
     `TransferError::Lifecycle(Conflict)`.
   - `orchestration.rs` classifies `TransferError::Lifecycle(_)` as not
     retryable. `recovery_scan.rs` then returns the error, the runtime exits,
     and `main.rs` shuts the controller down.
   - `videnoa-controller-task-20.md` says lifecycle CAS failures are meant to
     be fatal. A conflict caused by an authorized cancel is an expected
     outcome, not an invariant violation.
   - Upload completion (`FinishUpload`) has the same exposure.
   - Fix: map a nested `LifecycleError::Conflict` to a deferred or retryable
     outcome, so the next pass reloads the task and converges on the
     cancellation.
3. **High: editing a worker's endpoint redirects existing attempts.**
   - Where: `persistence/worker.rs` `update_worker`.
   - Recovery then polls the old job ID on the new machine. It gets a 404,
     and the task ends up permanently `remote_state_ambiguous`.
   - A download in progress can fetch a stale `{task_id}/output` from the new
     machine.
   - Fix: reject endpoint changes while the worker has attempts that depend
     on its remote state. Credential rotation stays allowed.
4. **Medium: the login limiter does not count concurrent attempts.**
   - Where: `auth/service.rs`, `auth/password.rs`.
   - Failures are recorded only after the Argon2 check. A burst of requests
     all pass the limiter before the first failures land, so they get far
     more than 5 guesses, and they queue without bound on the password
     semaphore.
   - `controller-bearer-rate-limiting.md` covers sequential requests only.
5. **Medium: the cancel confirmation carries over to another task.**
   - Where: `controller-web` `TasksPage.tsx`, `TaskDetailPane.tsx`.
   - `TaskDetailPane` is not keyed by task, so an open "Confirm Cancellation"
     from task A stays open after selecting task B. Confirming it cancels B.
   - Fix: add `key={selectedTaskId}`, and bind the confirmation to the task
     ID.

## Interop with today's worker changes

All four changes are compatible. Codex checked the normal controller flow by
reading the code; no interop tests were run.

- **Staged upload:** no target file means a fresh upload, and a matching stat
  size still counts as done.
- **Outputs published only on success:** the controller downloads only after
  `Completed` and never lists the worker's directories.
- **Cancel during finalization:** a job DELETE that returns 404 is accepted.
- **Output equals input is rejected:** `input.*` and `output.*` always have
  different names, so this never triggers for controller jobs.
