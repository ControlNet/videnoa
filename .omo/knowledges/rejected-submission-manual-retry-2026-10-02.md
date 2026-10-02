# Manual retry of rejected submissions (2026-10-02)

A Worker HTTP 400 on `POST /api/run` fails the task with
`remote_submission_failed` at stage `submission`, `retryable=false`. Automatic
retry stays blocked (resubmitting unchanged input to an unchanged Worker
repeats the rejection), but the task detail's Retry action now resumes it.

## Design

- `Lifecycle::retry_mode` (`lifecycle/classification.rs`) maps
  `(RemoteSubmissionFailed, Submission)` to `Resume(ResumeStage::Staged)`
  before the `retryable` check, like the `input_changed` precedent. Other
  stages stay `Blocked`.
- `ResumeStage::Staged` -> `TaskStatus::Staged`, durable action `Submit`.
  The reconciler then runs the normal `Staged` path: admission,
  `StartSubmission`, claim, `run`.
- Same attempt, same `submission_key`, same remote input path; upload is not
  repeated (`bind_upload` would refuse anyway because `remote_input_path` is set).
- `retry_lifecycle_stage` clears `task_attempts.submission_owner` when the target
  is `staged`. Without this the same Controller process's claim
  (`submission_owner != ?`) reports `Owned` forever and the task never
  resubmits; mutation-checked by
  `task20 remote_isolation::rejected_submission_resubmits_on_manual_retry_without_reupload`.
- UI: `controller-web/src/tasks/taskActionPolicy.ts` mirrors the pair in
  `canRetryTask` and gives `stage_retry` guidance.

## Idempotency note

The Worker validates the workflow before `claim_idempotent_job`
(`crates/core/src/server/mod.rs`, `run_workflow_by_name`), so a 400 records no
key and resubmitting the same key after the fix creates the job. The mock
Worker e2e test checks the Controller side: two Run requests with the same key,
one job, one upload.
