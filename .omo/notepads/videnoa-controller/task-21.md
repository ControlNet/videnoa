# Task 21 Notepad

## 2026-09-03 Adversarial Slice

- The 20,000-attempt detail blocker is resolved: `GET /api/tasks/{id}` now uses the shared bounded page request and returns total/limit/offset metadata in stable `attempt_no DESC, id DESC` order.
- The default 100-attempt response is 63,623 bytes; explicit pages are capped at 500, and the frontend loads older pages only through the operator-controlled next-page action.
- Mixed same-key races are deterministic with a start barrier: whichever canonical body wins receives one create and seven replays, while all eight requests for the other body conflict; only one task is durable.
- Two concurrent publication finalizers converge safely: exactly one completion, exact final bytes, and completed durable lifecycle.
- Existing Task 11-14 fixtures compose cleanly into dedicated Task 21 targets without copying or editing production code.
- Linux cross-filesystem coverage deliberately selects different device IDs and proves destination-owned staging; it does not inject an actual EXDEV return and does not claim Windows-native execution.
- Secret Guard baseline findings are pre-existing fixture literals and generic ignore gaps, not Task 21 leaks.

## 2026-09-04 Backend Blocker Red/Green

- Attempt-page red: a WAL progress-handler checkpoint committed attempt 101 after `COUNT(*)` established a 100-row snapshot; the old implementation returned `total = 100` with one item at offset 100. Green: count and ordered page selection now share one deferred SQLx read transaction, and the same test returns `total = 100` with an empty page.
- Canonical intake ownership red: temporarily perturbing persisted priority made the strengthened mixed-body race fail with durable/response priority `8` versus winning request priority `7`. Green: ten repetitions identify the sole `201 Created` body, require all seven `200 OK` replays to match it, and assert the durable request exactly matches its differing priority, source, and source-reference fields.
- SSE disconnect red: before socket shutdown, a write on the first TCP stream still succeeded. Green: the test consumes initial `refetch`, forces `Shutdown::Both` and proves writes fail, commits a historical worker update while disconnected, reconnects for a second initial `refetch`, then receives only the new durable version-1 worker update.
- Eight consecutive runs of the snapshot, mixed-intake, SSE reconnect, and duplicate-publication race cases passed without sleeps or wall-clock polling.
- Final Task 21 targets pass `7 + 18 + 22 + 1 + 46` tests; persistence history/query-plan/repository, task API, Task 14 SSE, and all 21 Task 20 orchestration tests also pass. Formatting, strict all-target/all-feature Clippy, all-target build, LSP diagnostics, diff whitespace, and the 250 pure-LOC audit are clean.

## 2026-09-03 Frontend Red Phase

- `npm test -- src/tasks/useTaskDetail.test.tsx` reproduced both reviewer races before production edits: delayed task A history replaced selected task B (`...0022` became `...0021`), and a stale history page after SSE reload rolled detail version 2 back to version 1.
- The mechanism was threefold: load-more had no AbortController, no selected-task/generation/offset ownership, and no attempt-ID deduplication or total bound.

## 2026-09-04 Frontend Green Phase

- `useTaskDetail` now owns each history request by selected task, detail generation, and offset; task changes and reloads abort pending work, while identity checks reject abort-resistant late completions.
- Accepted history pages preserve newest-to-oldest order, deduplicate attempt IDs, and clamp the rendered list to the authoritative total without allowing stale requests to clear newer loading or error state.
- The attempts section exposes `aria-busy` and a polite loaded/total announcement; history failures remain assertive and keyboard-retryable.
- The dedicated production-preview suite passed all five Chromium scenarios, including selection and SSE races, failure/retry, synchronous repeated activation, desktop loading, 375px reflow, axe analysis, and empty browser storage.
- Final frontend gates passed: 104 Vitest tests, TypeScript project checking, ESLint, and the Vite production build.
- Fresh visual evidence uses an installed Japanese CJK fallback on the Linux capture host; the Task Detail font stack also names common Korean platform fonts for hosts that provide them.
- Two fresh independent visual reviewers passed the final desktop loading and 375px internally-scrolled history captures with no blockers; the browser suite also asserts that the sticky selected-task header remains within the pane bounds.

## 2026-09-04 Visual Evidence Revisit

- Desktop task-row edge: evidence framing, not page clipping. Browser geometry proves `documentOverflow=false` while the bordered task table has `scrollWidth > clientWidth` and `overflow-x:auto`; paired left/right captures show only table columns moving.
- Remote Input/Output labels: evidence framing caused by the focused load-more control scrolling inside the bounded detail pane. The final inspector-region captures align both labels below the sticky header and keep the control visible.
- Narrow bare `23.8` and absent upper fields: intentional internal history scrolling presented ambiguously by the old single capture. The final packet pairs a 375px top/context view with a separate history view whose first visible value retains its `FPS` label and whose sticky selected-task header remains in bounds.
- Missing visible focus: capture-method defect. Programmatic focus did not prove `:focus-visible`; the final scenario uses keyboard traversal back to the load-more button, asserts a solid computed outline, and captures the visible violet ring before activation.
- Duplicate loading copy is intentional and non-overlapping: the polite live status announces asynchronous progress while the disabled button communicates the pending state of the initiating control.
- Complete fresh packet: `task-table-desktop-left.png`, `task-table-desktop-right.png`, `task-detail-desktop-focus.png`, `task-detail-desktop-loading.png`, `task-detail-375-top.png`, and `task-detail-375-history.png`. Every PNG is newer than the capture-code edit; no trace, video, HAR, storage-state, cookie dump, or error-context artifact exists.
- Independent visual integrity reviewer: PASS, high confidence (`0.96`), 6/6 coverage, no blockers.
- Independent visual precision reviewer: PASS, high confidence (`0.94`), 6/6 coverage, no blockers.

## 2026-09-04 Error / Logs Reachability Remediation

- Atlas's remaining evidence blocker reproduced as a browser geometry failure: the prior desktop focus position was not at the final pane scroll range, left the Error / Logs ending one pixel below the pane boundary, and provided no safe bottom clearance.
- The capture flow now settles desktop focus, desktop loading, and narrow history within the final four pixels of the detail pane's vertical scroll range. Executable assertions require the pane to own `overflow-y:auto`, the final Error / Logs node to remain below the sticky header, the complete node to be visible, and at least 12 CSS pixels of bottom clearance.
- The four-pixel evidence inset preserves complete Remote Input/Output labels in both desktop captures while retaining the visible keyboard focus ring and distinct loading live-status/disabled-control state.
- The final six-file inventory is unchanged: paired desktop task-table left/right positions, desktop detail focus/loading states, and complementary 375px top/history states. All six PNGs were regenerated by the final passing Chromium run after the capture-code edits.
- Final dimensions and signatures: table captures `1280 x 720`, desktop detail captures `888 x 545`, and narrow captures `375 x 812`; all are valid non-interlaced RGB PNGs with coherent task data and intact Japanese/CJK paths.
- Required gates pass: `npm test -- --run` (`104/104`), `npm run lint`, `npm run build`, and `npx playwright test --config=playwright.task21.config.ts --project=chromium --workers=1` (`5/5`). LSP diagnostics and scoped diff whitespace are clean.
- No trace, video, HAR, storage-state, cookie dump, failure-context, or stale superseded screenshot artifact exists in the Task 21 evidence directory.
- Independent design-system and functional-integrity reviewer: PASS, high confidence, 6/6 coverage, no findings or blockers.
- Independent visual-fidelity and CJK-precision reviewer: PASS, high confidence, 6/6 coverage, no blockers; explicitly confirmed complete Error / Logs endings with safe bottom clearance in both desktop detail images and the narrow history image.

## 2026-09-04 02:54:03 +10:00 Final Evidence Inventory Correction

- Corrected `security-filesystem.txt` to match the final repository inventory: 10 files total, including six PNG screenshots, `load-concurrency.txt`, `security-filesystem.txt`, `playwright/html/index.html`, and `playwright/results/.last-run.json`.
- The six PNGs comprise two desktop task-table captures (`task-table-desktop-left.png`, `task-table-desktop-right.png`), two desktop task-detail captures (`task-detail-desktop-focus.png`, `task-detail-desktop-loading.png`), and two narrow task-detail captures (`task-detail-375-top.png`, `task-detail-375-history.png`).
- The browser suite remains five scenarios. Screenshot count and scenario count are separate, and repeated temporary runs did not regenerate repository reports.
