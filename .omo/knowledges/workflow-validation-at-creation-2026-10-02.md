# Workflow validation before task creation (v0.1.9, branch `feat/workflow-validation`)

## Problem

Controller only checked a workflow's interface (`input`/`output` Path ports).
A workflow whose graph the Worker rejects (unknown node, bad params, invalid
JSON) was still scheduled; the task failed only at `POST /api/run`, after the
input upload.

## Latent bug found on the way

Saved Worker workflows are *listed* as `anime.json` (`WorkflowEntry.filename`)
but `POST /api/run` requires the file stem (`anime`) and rejects names ending
`.json` with 400 "workflow_name must not include .json suffix". Controller sent
the listed name, so every task using a saved workflow (not a preset) was
rejected. The mock Worker accepted `.json` names, so tests never caught it.
Fix: `WorkflowName::run_name()` strips `.json`; used by the run request and by
the recovery match against `job.workflow_name`. The mock now enforces the same
rule and tests submit `eligible-workflow` (stem).

## Design

- Worker: `POST /api/run/validate` with `{"workflow_name": ...}` runs the exact
  resolve/parse/validate path of `/api/run` (`load_run_workflow`) and answers
  204, or 400 with the same `{"error": ...}` text, or 404 when not found. No job
  is created. Old Workers (v0.1.5-v0.1.8 checked) answer 404 through the
  `/api/{*path}` catch-all, not SPA HTML.
- Controller `VidenoaClient::validate_run` -> `RunValidation::{Valid,
  Invalid{reason}, Unknown}`. `capabilities()` validates every
  interface-eligible entry; `Invalid` marks it Incompatible and records the
  bounded reason; `Unknown` (404) and errors (5xx, timeouts, 401) keep it
  eligible and only log a warning. A validation failure never fails the probe.
- Persisted as `WorkerCapabilities.invalid_workflows` (serde default, omitted
  when empty). Rollback needs the pre-upgrade snapshot anyway (docs), because an
  older Controller's `deny_unknown_fields` would reject the new key.
- Task create, batch preview and batch create return 400 field error
  `workflow`/`invalid_value` only when some worker reports the workflow invalid
  and no worker lists it as runnable. Unknown workflows still queue. The check
  runs after the idempotency replay lookup, so replays are unaffected.
- controller-web: schema `invalid_workflows` defaults to `[]`; the Workers
  table error cell shows "N invalid workflows" with `name: reason` lines in the
  title.

## Tests

- Worker: `run_validation_matches_run_without_creating_a_job` (core lib).
- Client: `run_validation_distinguishes_valid_invalid_and_unsupported`,
  `capabilities_exclude_workflows_the_worker_would_reject`,
  `capabilities_keep_workflows_eligible_when_validation_is_unavailable`.
- Runtime: `task20 worker_health::probe_records_workflows_the_worker_would_reject`.
- API: `task_api workflow_validity::*`.
- Mock: `Fault::InvalidWorkflow { name, error }` (persistent, by run name)
  makes both `/api/run/validate` and `/api/run` answer 400.

## Gotcha

`paths::publication_tests::cross_filesystem_rename_is_typed_and_leaves_media_empty`
fails when `TMPDIR=/dev/shm/...`: it needs `/dev/shm` to be a different
filesystem from the temp dir. Use `--no-fail-fast` or the default TMPDIR when
running the whole controller suite.

## CI gotcha: `cargo fmt` misses include!-ed Controller modules

Most Controller modules are wired with `include!` (`module_topology.rs`), which
`cargo fmt --all` does not follow. CI's quality gate also runs rustfmt on every
tracked file and stopped the item 6 and 7 merges at that step (so their clippy
and tests never ran in CI). Run the CI command locally before pushing:

```bash
git ls-files -z '*.rs' | xargs -0 -n1 rustfmt --edition 2021 --check
```
