# Controller Web Task Filters and Columns

## Stable Contract

- Source values are `manual` and `api`; the task list query parameter is `source`.
- Failure Stage values come from `failureStageSchema`; the task list query parameter is `failure_stage`.
- `TaskQuery` owns URL persistence, while `taskPagePath()` forwards only server-owned filters to `api/tasks`.
- Live `task_updated` membership must apply Source and Failure Stage in `matchesTaskQuery()` so SSE changes cannot bypass the current server view.

## Optional Columns

- Stable column IDs are `input_path`, `output_path`, `attempts`, `duration`, `failure_stage`, `failure`, `error`, and `remote_job_id`.
- The generic `path` ID is intentionally absent because it obscures whether a value is an input or output path.
- Checkbox accessible names use `Show <label> column`; tests selecting form fields should constrain by textbox role rather than fuzzy label text.
- The Columns overlay must stack above both sticky headers: picker `z-index: 3`, detail header `z-index: 2`, table headers `z-index: 1`.

## Density and Overflow

- Task rows use the shared `--control-compact` token and render at 36px.
- Long task content remains inside `.task-table-frame`; the document itself must not gain horizontal overflow.
- Horizontal table navigation remains visible and keyboard-operable at desktop, tablet, and mobile widths.

## Verification Evidence

- ESLint passed.
- Vitest passed: 21 files, 116 tests.
- TypeScript and Vite production build passed.
- Playwright Chromium passed: 50 tests.
- The intercepted-pointer regression was reproduced with `<th>Progress</th>` blocking `Show Attempts column`, then passed after the picker stacking fix.
- Manual Chromium measurements at 1280x900, 768x900, and 390x844: `rowHeights=[36]`, `documentOverflow=false`, `tableOverflow=true`.
- The final single-Chinese-task regression clicks all eight toggles with detail open. A second red run proved z-index 2 was still intercepted by the detail header; z-index 3 resolves both header layers.
- Fresh browser console reported zero errors and warnings.
- Two final independent reviewers inspected all seven live Chinese-task captures after the z-index 3 fix and returned PASS with no blockers. Full lint, 116 unit tests, build and 50 Chromium tests passed again on that source.

## Artifacts

- Final screenshots: `.omo/evidence/controller-correction-live/`.
- Automated browser tests use deterministic mock data. Final manual QA uses the actual Controller HTTP service and a persisted task with synthetic Chinese-named input, not production video or GPU execution.
- Manual `uncheck()` initially checked state before the URL-driven React update settled. Explicit clicks followed by waiting for the resulting checkbox state verified all eight options twice, row height 36, picker z-index 3, and no document overflow; console errors and warnings were zero.
