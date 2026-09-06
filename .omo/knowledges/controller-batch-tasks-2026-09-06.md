# Controller batch task intake

## Scope and implementation

Continued interrupted Codex session `01a07200-280e-7b71-9541-88044f061dd2` from
`dev` at `8499ca7`. The interrupted changes contained an unfinished Rust preview
endpoint and wildcard scanner. The active request was batch intake; older Docker,
HTTP compatibility, path architecture, and autocomplete work had already landed.

- Tasks toolbar adds Add Batch beside Add Task, preserving the existing UI tokens.
- Input Pattern, Output Directory, and Workflow reuse ManualTaskField. Batch fields
  have separate DOM ID prefixes so the mounted manual dialog does not collide.
- Glob matching supports *, ?, character classes, and recursive ** components.
  Relative patterns resolve under the workspace; absolute patterns retain their
  OS namespace. No Docker path translation is added.
- `POST /api/tasks/batch-preview` uses authenticated mutation middleware, including
  session Origin and CSRF validation. Preview runs blocking filesystem work outside
  the async executor, performs no writes, and reads metadata instead of hashing
  entire video files. Intake still captures full file content identity.
- Scans use no-follow directory capabilities, exclude private state and symlinks,
  sort/deduplicate matches, and reject scans over 20,000 entries, 500 files, or 64
  directory levels instead of returning incomplete results.
- Outputs use the original stem, a validated middle extension, and original
  extension. Original filenames require directory mode. Preview flags existing,
  unsafe, and duplicate outputs. Successful output validation supplies absolute
  normalized paths for submission.
- Creation uses existing POST /api/tasks, one stable key per preview row. It pauses
  on the first failure, preserves successful rows, and retries remaining rows with
  identical keys/bodies. Settings lock once creation starts. Batch execution is
  not transactional. Closing/reloading the dialog discards browser retry state.
- LAN HTTP uses the existing getRandomValues-based idempotency key generator.

## Verification and test details

Fixtures are explicitly synthetic; no real media jobs are submitted by these tests.
The Task API tests reuse the existing low-cost authentication fixture. No production
password cost changes or test-runtime regressions were introduced.

The shared Playwright task fixture needed default workers and path-suggestion routes:
without them, the latest worker-name hook could reach Vite's live API proxy and its
401 response redirected the simulated session to login. Per-test routes can override
the defaults. Native option disabled state is checked through the DOM property
because this Playwright version's toBeDisabled assertion did not recognize it.

Rust module declarations are included via module_topology.rs. Cargo fmt alone does
not traverse the newly added production modules; format/check them explicitly.

Commands (run from repository root unless noted):

```bash
cargo +1.83.0 fmt --all -- --check
rustfmt +1.83.0 --edition 2021 --check crates/controller/src/tasks/batch.rs crates/controller/src/paths/batch.rs
cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.83.0 test --locked -p videnoa-controller --all-targets
cargo +1.83.0 test --locked -p videnoa-controller --test task_api --test task_api_concurrency
npm --prefix controller-web run lint
npm --prefix controller-web run test
npm --prefix controller-web run build
(cd controller-web && npx playwright test tests/e2e/batch-tasks.spec.ts tests/e2e/task-creation.spec.ts tests/e2e/task-completion.spec.ts)
```

Frontend verification: 139 unit tests passed; lint/build passed; both batch browser
tests and six existing creation/completion browser tests passed. Desktop batch
Axe check found zero violations. Screenshots were inspected at:
`.omo/evidence/controller-batch-tasks/preview-desktop.png` and `preview-mobile.png`.
Full Controller suite log: `/tmp/videnoa-batch-controller-tests.log`.

Full Controller suite: 470 passed, 0 failed, 1 ignored across 49 test binaries.
Final strict Clippy passed. After extracting shared workflow/priority validation,
the 17-test Task API and one concurrency regression passed again.

## Create Tasks disabled conditions

At f5fd20c, the button uses `disabled={busy || !canCreate}`. `canCreate`
requires non-null preview rows, at least one row, every preview `row.error` null,
and not all rows created. Every option edit clears the rows and requires another
Preview, including workflow/priority changes. Preview errors leave rows null.
A single conflicting row blocks the entire batch; there is no skip-conflicts
selection. Submission errors use a separate `submissionError`, so after creation
pauses they do not disable Retry Remaining. All-created replaces the button with
Done. The disabled button has no dedicated explanation/tooltip; users must infer
its cause from preview status or alerts.

## Two-step batch dialog follow-up

The user requested an explicit input -> preview -> creation flow. BatchTaskDialog
now renders Add Batch with only a primary Preview Tasks submission. Successful
preview (including zero matches/conflicts) switches the same native dialog to a
Preview Tasks screen; settings are replaced by a pattern/workflow/priority summary
and the task table. Only the review screen offers Create Tasks. Back retains the
options, discards the old preview, and focuses Input Pattern. Preview failures keep
the input step open. Review navigation focuses the heading for keyboard/screen
reader users. Starting creation disables Back to retain the existing stable per-row
idempotency keys and partial-failure retries.

Validation: frontend lint/build and all 139 unit tests passed. Three Playwright
batch tests passed, covering step visibility, keyboard submission, preview failure,
Back preserving settings, re-preview, zero matches/conflicts, mobile overflow,
partial-success retry keys on HTTP, and the desktop Axe audit. Screenshots at the
existing batch evidence paths now show the separate preview screen. No backend
code or API behavior changed; Rust tests were not rerun for this UI-only change.

```bash
npm --prefix controller-web run lint
npm --prefix controller-web run test
npm --prefix controller-web run build
(cd controller-web && npx playwright test tests/e2e/batch-tasks.spec.ts)
```

## Reversible preview row removal

Based on dev at 6632115, preserving the user's recent microcopy/style edits.
Each preview row now has a sticky Actions column with Remove/Restore buttons.
Removed rows remain in the table with muted text, struck-through paths, and a
Removed status. A browser-only `excluded` flag controls selection. Submission and
retry skip excluded rows; selected counts drive conflicts, progress, completion,
and empty-selection disabling. Row toggles lock once creation starts to preserve
submission identity. Back/re-preview resets selection alongside the preview.

Preview API rows now additionally expose `validation_error` (the independent
path/naming error before duplicate detection) and `output_key` (the server's
platform-aware path comparison key). Existing `error` remains the full-preview
error. These separate fields are necessary because excluding one duplicate should
clear the duplicate conflict without erasing an underlying output-exists or unsafe
path error. Frontend duplicate detection uses selected rows only. The new fields
are optional in the client parser so responses from older servers retain their
original safety errors rather than being reclassified by message text. No intake,
filesystem-write, authentication, or database behavior changed.

Validation passed:
- 143 frontend unit tests, including duplicate remove/restore, independent path
  errors, server-supplied path equivalence, and legacy response handling.
- 5 batch browser tests, including removal/restoration, all-removed state,
  keyboard restore, selected-only creation and lost-response replay, duplicate
  resolution, and mobile Axe audit. Fixtures are synthetic; no real media tasks.
- 17 Task API tests plus one intake concurrency regression. The backend test
  verifies duplicate errors cannot hide the preserved output-exists error.
- Frontend lint/build, Rust fmt (including direct module check), strict all-target
  Controller Clippy, and diff whitespace checks.

```bash
npm --prefix controller-web run lint
npm --prefix controller-web run test
npm --prefix controller-web run build
(cd controller-web && npx playwright test tests/e2e/batch-tasks.spec.ts)
cargo +1.83.0 test --locked -p videnoa-controller --test task_api --test task_api_concurrency
cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
```

Inspected screenshots: `.omo/evidence/controller-batch-tasks/removed-row-desktop.png`
and `removed-row-mobile.png`. Full backend suite was not rerun for this narrowly
scoped response-metadata and browser selection change.
