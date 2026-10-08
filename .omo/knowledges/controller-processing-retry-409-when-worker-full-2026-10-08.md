# Controller: "Retry Failed Stage" returns 409 when the Worker has no free slot (2026-10-08)

## Symptom

A task failed with `processing_failed` (stage `processing`, CUDA failure 100
on nectar3 after the GPU was lost). The Task Detail pane offers
*Retry Failed Stage* and shows *Manual Retry: Available*, but clicking it
only shows "The task changed before this action completed ..." and the task
stays `failed`.

## Cause

- `POST /api/tasks/{id}/retry` for `processing_failed` takes the
  `RetryMode::NewProcessingAttempt` path (`operations/tasks.rs`
  `processing_retry`). It checks that the remote job is terminal, deletes the
  remote workspace, then calls `retry_processing_attempt`
  (`persistence/lifecycle_retry.rs`).
- That single `UPDATE tasks ... WHERE` is a CAS that also requires:
  - the original Worker is `enabled = 1 AND online = 1`;
  - the scheduler is not paused (`? = 0` bound to `policy.paused`);
  - the Worker has admission room: pending tasks
    (`reserved`/`uploading`/`staged`) on it `<`
    `max(compute_slots - active(submitting/processing), 0) + prefetch_per_worker`.
- Any of these failing returns `CasOutcome::Conflict`, which maps to
  `LifecycleErrorCode::Conflict`, then HTTP 409 with
  "task changed since it was read".
- The WebUI (`controller-web/src/tasks/TaskDetailPane.tsx`) shows the same
  generic "task changed" text for every 409 and ignores the server message.
- With the defaults (`compute_slots = 1`, `prefetch_per_worker = 1`), a Worker
  running one job with one more task already staged has no room. Here the
  retry is pinned to the failed attempt's Worker, so new ani-rss tasks keep
  taking the slot first.

## Workarounds

- Retry when the Worker has no task in `reserved`/`uploading`/`staged`.
- Or temporarily raise *Prefetch per Worker* in the Controller Settings, retry,
  then set it back.
- Pausing the scheduler does not help; a paused scheduler also blocks it.

## Possible fixes (not done)

- Return a distinct error (Worker offline / no free slot / scheduler paused)
  instead of the version-conflict CAS outcome.
- Show the server's 409 message in the Task Detail pane.
- Or let a processing retry go back to the queue instead of requiring the
  original Worker to have room immediately.
