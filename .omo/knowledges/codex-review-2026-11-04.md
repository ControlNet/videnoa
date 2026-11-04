# Codex review after v0.1.11 (2026-11-04)

Read-only Codex review (`codex exec -m gpt-6-astra -s read-only`, reasoning
effort high) of `dev` at 55b22e5, asked to skip the items already recorded in
`design-review-2026-10-30.md`, `post-v0.1.10-review-2026-10-29.md` and the
2026-10-23 Codex reviews. The sandbox blocked writes even under `/tmp`, so
Codex could not build; it ran one SQLite probe. Each finding below was
re-checked against the code by Claude. Nothing has been fixed yet.

## 1. Startup sweep deletes staged files by stem only (regression, ead04bc)

- `server/staged_outputs.rs` `sweep_beside` / `is_staged_file_of` delete any
  `.<stem>.<6 alnum>.videnoa-<direct|video|mux>[.<ext>]` beside the output of
  a job that was running when the worker stopped. Ownership is the output
  stem only; extension and job are not checked.
- `nodes/video_output.rs` `keep_encoded_video` deliberately keeps the encoded
  video (`.<stem>.<rand>.videnoa-video.<ext>`) after a failed mkvmerge and
  names it in the error so the user can recover it.
- Scenario: mux fails for `/media/Episode.mkv` (encode kept), the user reruns
  the same output (or `Episode.mp4`), the worker is killed or restarted while
  that job runs; the next start deletes the kept encode too. Also: two
  workers with separate data dirs writing same-stem outputs into one folder
  can delete each other's active staged file.
- Existing test `a_restart_removes_staged_outputs_of_jobs_that_were_running`
  only keeps a file with a different stem.
- Fix direction: record the exact staged paths each job creates (job store)
  and delete only those; give kept recovery files a name the sweep never
  matches.

## 2. Worker deletion does not bump task/attempt versions (regression, b0bd64f)

- `persistence/worker_registry.rs` `delete_worker` sets `worker_id = NULL`
  on terminal tasks and attempts without `version = version + 1`.
- `operations/tasks.rs` `retry` checks `attempt.worker_id.is_none()` on its
  snapshot; `persistence/lifecycle_retry.rs` `retry_lifecycle_stage` only
  checks status/version/discarded.
- A retry that read the rows before a concurrent delete commits succeeds and
  leaves a `downloading` task with no worker; recovery then fails it as
  `remote_state_ambiguous` (`recovery/reconciler.rs`). Codex confirmed the
  SQL interleaving on an in-memory copy of the v0.1.10 fixture.
- Impact is small: after deletion the task cannot be retried anyway; only the
  failure reason changes. Needs two near-simultaneous operator actions. Claude
  rates it Low (Codex: Medium).
- Fix direction: bump versions in the detach updates and require
  `worker_id IS NOT NULL` in the retry write.

## 3. Nested Workflow outputs collected from every inner node (pre-existing)

- `nodes/workflow_io.rs` `WorkflowNode::execute` iterates all inner nodes'
  outputs (`HashMap`, random order) and keeps any port whose name matches a
  declared interface output; the last one wins.
- An intermediate node with an output port of the same name (e.g.
  StringTemplate `value`) can therefore supply the nested workflow's result
  instead of `WorkflowOutput`. Same code in v0.1.10.
- The nested test uses a passthrough where the values coincide, so it cannot
  catch this.
- Fix direction: take results only from the inner `WorkflowOutput` node(s).

## Checked and found sound by Codex

Retry/discard per-task exclusion; `/api/run` idempotency receipt before
loading the workflow; unknown remote job statuses stay non-terminal; cancel
waits for the worker to stop before deleting the workspace; staged direct
outputs and the source-equals-output check.
