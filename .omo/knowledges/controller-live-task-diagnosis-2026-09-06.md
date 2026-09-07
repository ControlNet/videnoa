# Live Controller task diagnosis (read-only)

The user requested inspection of two manual Docker tasks: missing Processing
progress and remote completion followed by Controller failure. No container,
task, authentication record, output, or remote job was modified during diagnosis.
Separate untracked frontend design work was present and left untouched.

## Evidence

The running `videnoa-controller-test` uses image
`5ed9ed21390000b0aef167db0f4124631a44a2bf4af5902b17d030e51240cc0e`.
Its private data and media are two independent bind mounts at `/workspace/data`
and `/media`. Their device IDs agree, but mount IDs differ. This is the documented
same-device/different-mount EXDEV limitation, not an HTTP compatibility issue.

- Task `c0f23b7c-08d8-4230-b84f-0da0aaa39894` reached publication and failed with
  `publication_failed`, `atomic publication cannot cross filesystems`.
  Its remote job `6c56fd8d-913d-4abf-8a4d-a54a5c8b1836` still reports completed.
  The Controller already downloaded 46,102,664 bytes into
  `data/c0f23b7c-08d8-4230-b84f-0da0aaa39894/output.mkv.verified`, with an evidence
  sidecar. A fresh read-only SHA-256 and size check matched SQLite's persisted
  expected output evidence exactly. The final output is absent. Do not recompute
  or discard this verified result. Cross-filesystem capability failures are
  currently marked nonretryable; a mount change alone does not reset this failed row.
- Task `4a2f347b-74aa-44ed-b71d-8ede3da72610` failed during processing with
  `remote_state_ambiguous`, `durable remote job is missing from the worker`.
  Its recorded job `d6fd5539-ab0d-4bae-a5bc-570fae990df5` still returns HTTP 404.
  The available evidence does not establish why the remote record disappeared.
  Do not assume manual deletion or safely repeat compute without further evidence.

## Confirmed progress integration defect

`crates/controller/src/recovery/processing.rs` polls the remote job and checks its
identity, but handles Queued/Running by merely scheduling another Poll. It does
not consume `job.progress`, including on completion. Production code has no calls
to `Store::update_task_progress` or `Store::update_attempt_progress`.
Both live tasks and attempts retained the initial zero/null progress JSON.
The completed remote job supplied current_frame=2698, total_frames=2700,
fps=34.503235, eta_seconds=0.057965579399293476, proving the upstream field exists.

A future fix should persist paired task/attempt progress with appropriate CAS
and lifecycle concurrency handling, propagate versioned SSE updates, normalize
numeric fields safely, and directly test real Running poll updates. Frontend
rendering alone cannot repair missing backend progress. No implementation change
was made as part of this requested diagnosis.

Publication recovery must preserve no-clobber, input identity, final-artifact
identity, and the prohibition on media-visible intermediate files. The current
separate-mount layout needs an explicit resolution; do not silently copy into the
final filename, add sibling staging, or manually mark a task completed in SQLite.
