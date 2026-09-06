# Current Release Manual QA - 2026-08-27

## Scope

- Server: `http://127.0.0.1:39123`
- Disposable data: `/tmp/opencode/videnoa-security-poc`
- Browser: Playwright Chromium, primarily 1280x800
- Source and persistent configuration were not edited.
- No GPU workflow was launched.

## Verdict

Overall FAIL. The editor shell, navigation, jobs, models, settings, performance, API,
and CLI remain usable, but preview extraction is broken in the current release and
multiple P1 interaction/accessibility defects reproduce.

## P0

### PASS - Application shell and navigation

1. Open `/`.
2. Use the header links for Editor, Jobs, Models, Performance, and Settings.
3. Return to Editor.

Expected: every route loads and the shell remains responsive.

Actual: all five routes loaded. Header status moved between idle and workflow running
as expected. Final state was Editor, no modal, no editor nodes, and health 200.

### PASS - API and CLI basics

- `GET /api/health`: 200 `{"status":"ok"}`.
- `GET /api/jobs`: 200.
- `GET /api/models`: 200 with three models.
- `GET /api/config`: 200 and unchanged settings.
- `GET /models`: 200 HTML deep link.
- Unknown `/api/*`: bounded 404 JSON.
- `target/release/videnoa --help`: rendered top-level help.
- `target/release/videnoa run --help`: rendered run help.
- `target/release/videnoa run`: exited with the missing `<WORKFLOW>` error.

### PASS - Non-GPU job lifecycle

1. Submit a one-node `HttpRequest` workflow to a disposable local delayed endpoint.
2. Observe Jobs while the endpoint is waiting.
3. Let the request complete.

Expected: queued/running/completed states update without GPU work.

Actual: API returned 201 queued; the UI showed Running and then Completed in 20.178s.
The header returned to idle and job detail expansion worked.

### FAIL - Live preview extraction

1. Open Preview from the Editor toolbar.
2. Enter `/tmp/opencode/videnoa-security-poc/input.mp4`.
3. Click Extract Frames.

Fixture: H.264, 16x16, two frames, one second, 1,592 bytes.

Expected: at least one preview frame and a 201 response.

Actual: `POST /api/preview/extract` returned 500. The UI exposed the full FFmpeg
stderr ending with `Expected number for vsync but found: vfn`. No frames loaded.

Screenshot: `qa-preview-extract-failure.png`.

## P1

### FAIL - Retry offered for an active job

1. Submit the disposable delayed `HttpRequest` workflow.
2. Wait for the active card to show Running.
3. Inspect the same job in Job History.

Expected: active jobs expose Cancel only; Retry is terminal-state-only.

Actual: the active card exposed Cancel, while the Running history row simultaneously
exposed Retry and Delete. When the job became Completed, Retry disappeared.

Accessibility snapshot evidence included a Running row with `Retry` and `Delete`.

### FAIL - Numeric fields cannot be cleared

1. Add a Resize node to an empty editor.
2. Enter `640` in width.
3. Press Ctrl+A, then Backspace.
4. Replace the value with `320` as a control.

Expected: the field can temporarily become empty during editing.

Actual: Backspace immediately left the value at `640`. Replacing it with `320`
worked, proving the field was editable but could not represent an empty draft state.

### FAIL - Workflow selection cards are keyboard-inaccessible

1. Open Jobs > Run Workflow.
2. Inspect the accessibility tree and press Tab three times.

Expected: each workflow choice is a named button/radio/list option and tabbable.

Actual: all six choices were generic `DIV` nodes with no role or tabindex. The only
focusable element was Close, and Tab cycled on Close each time.

### FAIL - Default model cards are keyboard-inaccessible

1. Open Models in the default grid view.
2. Inspect card roles and the main-page focusable elements.

Expected: model cards are buttons/links or have equivalent keyboard semantics.

Actual: all three cards were `DIV`, role null, tabindex null, and absent from the tab
order. List view is a valid workaround: its rows are buttons, and Tab then Enter opened
the model detail dialog.

### FAIL - Icon-only editor controls lack accessible names

Keyboard traversal reached toolbar buttons whose accessible name, `aria-label`, and
title were all empty, including redo, auto-layout, fit, clear, load, and preview. The
node-palette collapse button was also unnamed. Tooltip text did not name the buttons in
the accessibility snapshot.

### FAIL - Numeric node controls lack accessible names

Resize width and height appeared as unnamed spinbuttons. Adjacent text was not exposed
as a label association.

### FAIL - Preview stale process response

Controlled timing fixture because live extraction is blocked:

1. Intercept preview extraction to return two distinct frame URLs.
2. Start processing frame 0 with a 1.5 second delayed response.
3. Press ArrowRight during processing to select frame 1.
4. Let frame 0's response complete.

Expected: changing frames cancels/discards frame 0's result or binds the result to
frame 0 only.

Actual: the UI reported `Frame 2 / 2`; the Before source was frame 1 (green), while
the After source was the delayed frame 0 result (blue). Network evidence was extract
201 and process 200.

Screenshot: `qa-preview-stale-response-failure.png`.

### BLOCKED - Live preview processing returns original frame

The candidate could not be exercised end to end because every live extraction attempt
failed before a preview session existed. A fake session correctly returned 404 from
`POST /api/preview/process`. Do not treat the controlled stale-response fixture as
backend processing evidence.

## P2

### PASS - Settings discard path

Changed `models_dir` to `models-qa-unsaved` without saving. The UI showed Unsaved
changes and enabled Save. Reset issued a second `GET /api/config`, restored `models`,
removed the dirty state, and disabled Save. No `PUT /api/config` occurred.

### PASS - Model detail stale-response guard

Delayed inspection for RealESRGAN by two seconds, closed it, and opened AnimeJaNai.
After the delayed response arrived, the dialog still showed AnimeJaNai and contained
no stale RealESRGAN filename.

### PASS - Preview error state

Extracting `/tmp/opencode/videnoa-security-poc/missing.mp4` returned 400 and displayed
`video file not found` inline without crashing the dialog.

### PASS - Performance disabled state

Performance loaded with telemetry disabled, explicit OFF states, an empty semantic
table, and repeated 200 responses for overview/export.

### FAIL - Completed-job WebSocket handshake noise

After the disposable job completed, the browser logged one unexpected error:
`WebSocket ... /api/jobs/<id>/ws failed ... 404`. The page recovered through polling,
so this did not block use.

### FAIL - CLI version flag absent

`target/release/videnoa --version` returned `unexpected argument '--version'`.

## Browser Console and Network

- Initial load: zero errors and zero warnings; one i18next informational message.
- Unexpected error: completed-job WebSocket handshake returned 404.
- Expected test error: missing preview path returned 400.
- Product failure: valid preview extraction returned 500.
- Core observed requests otherwise returned expected 2xx responses.

## Final State

- Browser closed.
- No dialogs open before close.
- No queued or running jobs.
- Both disposable delayed HTTP listeners closed.
- Final `GET /api/health`: 200 `{"status":"ok"}`.
