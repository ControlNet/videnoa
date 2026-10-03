# Downloader isolation and job-end cleanup (2026-10-02)

Fixes the P1 from `project-review-2026-09-26.md` ("Downloader results collide
by filename"), deferred until the owner chose job-end cleanup.

## Design

- `ExecutionContext.scratch: ExecutionScratch` (`crates/core/src/node.rs`).
  Each top-level context owns one lazily created root,
  `temp_dir()/videnoa/downloads/<uuid>`, removed when the last clone drops.
  `allocate_dir()` hands out `<root>/<n>` per download.
- Top-level contexts are built by `SequentialExecutor::execute_with_context*`
  (server `run_job`, `videnoa run`) and the preview loop
  (`server/preview.rs`). The video pipeline runs to completion inside
  `execute_with_context_and_debug_hook`, so the drop at its return is "job end".
- Nested contexts must share the outer scratch, or files from a sub-workflow
  are deleted before the outer workflow reads them:
  `executor.rs` (`execute_with_params_and_debug_hook`, `scratch:
  outer_ctx.scratch.clone()`) and `nodes/workflow_io.rs` (Workflow node).
  `nested_executions_keep_downloads_until_the_outer_job_ends` fails if the
  Workflow-node propagation is removed (mutation-checked).
- The Downloader keeps the original sanitized file name inside its own
  directory, so PathDivider-derived names are unchanged; only the parent
  differs per download.

## Behaviour changes

- Downloaded files no longer survive the job. A workflow that only downloads
  and returns the path leaves nothing behind; process the file within the
  workflow. Documented in README "Workflow constraints".
- Previously files were overwritten in a shared directory and never deleted;
  existing files there from older versions are left untouched.
- A crash or kill mid-job leaves that job's `<uuid>` directory behind (no
  startup sweep).

## Controller impact

None on the contract: the Controller never references the Downloader or its
directory, passes `input`/`output` explicitly and transfers only
`<task-id>/input|output.<ext>` through `/api/files`. `videnoa-controller` does
not depend on `videnoa-core`. Concurrent Controller tasks on one Worker
(compute slots > 1) running Downloader workflows no longer overwrite each
other.
