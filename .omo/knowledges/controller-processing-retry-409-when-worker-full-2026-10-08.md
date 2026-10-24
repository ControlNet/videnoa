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

## Workarounds (Controllers before the fix)

- Retry when the Worker has no task in `reserved`/`uploading`/`staged`.
- Or temporarily raise *Prefetch per Worker* in the Controller Settings, retry,
  then set it back.
- Pausing the scheduler does not help; a paused scheduler also blocks it.

## Fix (branch `feature/retry-queue-worker-choice`, merged into dev 2026-10-08)

- Processing retry now requeues: after the terminal check and workspace delete
  on the original Worker, `requeue_failed_task` sets the task `queued`,
  `worker_id = NULL`, `requested_worker_id = <Worker>` (migration `0014`,
  `ON DELETE SET NULL`). No capacity, online or paused check at retry time; the
  scheduler (`SCHEDULER_CANDIDATE_SQL`) and `reserve_task` only pair the task
  with `requested_worker_id`, and reservation clears it and creates attempt N+1.
- `POST /api/tasks/{id}/retry` takes optional `worker_id`. Processing, upload
  and rejected-submission failures can move to another Worker (the original's
  workspace is deleted first; unreachable original = refused). Later stages
  reject `worker_id` with a field error. `RetryTaskResponse.attempt_id` is
  `null` for a requeue.
- Cancelling a queued task ignores its old failed attempt
  (`lifecycle/cancellation.rs`), otherwise `attempt_cas` saw `failed != queued`.
- WebUI: split button *Retry* + arrow menu (`RetryWorkerMenu.tsx`); 409 shows
  the Controller's message instead of always "task changed".
- A task requested for a Worker that stays offline or disabled waits in the
  queue; cancel it or delete the Worker to release it.
