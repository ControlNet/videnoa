# Controller Web Final Frontend Preflight Knowledge

Date: 2026-09-04

## Deterministic Playwright mutation assertions

Playwright `locator.click()` waits for the browser actionability and click dispatch, but it does not guarantee that a Node-side `page.route()` callback has completed before the next synchronous assertion. Tests that assert a route journal must create the network waiter before the click and await it afterward:

```typescript
const response = page.waitForResponse("**/api/resource/action")
await page.getByRole("button", { name: "Confirm" }).click()
await response
expect(journal).toEqual(expected)
```

A temporary delay inside the route handler is an effective root-cause toggle: the old immediate assertion failed deterministically, while the response-waiting assertion passed repeatedly under the same delay.

## Shared-worktree browser verification

When repository Playwright configs write to historical evidence paths, create a per-run mirror under `/tmp/opencode`, exclude `node_modules` and `dist`, and link the repository `node_modules`. Run production-preview browser suites there. This preserves the shared dirty worktree and prevents unrelated evidence churn while still executing the current source.

## Responsive screenshot evidence

- A fixed viewport screenshot is not sufficient if the relevant nested scroll owner is below the fold. Scroll the table frame into view before capture.
- For wide CJK tables, align the exact error/path column rather than scrolling blindly to the far right. This makes truncation, glyph rendering, and containment directly reviewable.
- Validate every PNG signature and exact dimensions, then open every image. This run caught two evidence-only defects before review: a narrow screenshot that omitted the table and a synthetic query string with a secret-like label.

## Native dialog consistency

Native `<dialog>` elements do not inherit the task dialog's containment rules. Shared operational dialogs need all four guarantees together: `margin: auto`, a viewport-safe `max-block-size`, internal `overflow: auto`, and `overscroll-behavior: contain`. A width rule alone allowed the worker dialog to render flush against the top-left in production-preview Chromium.

Modal action buttons with inline icons should explicitly use the existing spacing token. `display: inline-flex` does not create icon/text separation by itself; the manual Create Task action needed `gap: var(--space-2)`.

## Console and network classification

Expected HTTP boundary failures can produce Chromium resource console messages even when the UI handles them correctly. Assert the exact expected message and status instead of either failing every console entry or globally ignoring console errors. This run allowed only the known synthetic 401 login bootstrap and 400 manual-validation responses; page errors and failed requests remained empty.

## Browser invariants confirmed

- Authentication material remained in HttpOnly/session protocol handling; browser local and session storage stayed empty.
- The 20,000-task surface rendered one bounded 50-row page and requested only bounded limits.
- Table horizontal overflow stayed owned by the table frame at 1440, 1024, and 375 widths.
- ArrowLeft, ArrowRight, Home, and End behavior is covered by the configured suite; hands-on QA directly rechecked ArrowRight, Home, and End.
- CJK paths and diagnostics used intentional ellipsis without document overflow or missing glyphs.
- Workers, settings, and manual intake restored or moved focus to the intended committed controls and alerts.

## Stable warning classification

The current production build emits non-fatal Rollup annotation-position warnings from Zod v4 `core/regexes.js` and `core/util.js`. The warnings remove comments only; the build exits 0 and emits the expected bundle.

## Evidence location

Final frontend evidence is under `.omo/evidence/videnoa-controller/final-preflight/frontend-browser/`.

## Disabled operational controls

Native `disabled` semantics do not guarantee a perceptible unavailable state. In this interface, the overflow arrows and pagination actions inherited active text, surface, and border styling plus a global `wait` cursor, so screenshots made unavailable controls look actionable.

Keep unavailable task controls local to their owning surface and distinguish them through multiple token-driven channels: quiet icon/text color, recessed background, subtle border, reduced opacity, and `cursor: not-allowed`. Do not hide them, alter their 2.25 rem dense dimensions, weaken the native `disabled` attribute, or change global loading cursor semantics for unrelated controls.

Browser regressions should compare computed styles between unavailable and available peers at both boundaries. Assert native enabled/disabled state first, then require different color, background, border, opacity, and cursor values. Cover both overflow directions and both pagination ends; a single first-page assertion misses the reversed final-page state.
