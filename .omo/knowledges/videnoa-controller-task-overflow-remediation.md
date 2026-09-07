# Videnoa Controller Task Overflow Remediation

Date: 2026-09-04

## Contract

- The Tasks table frame owns horizontal overflow; the route and document do not.
- Whenever the rendered table width exceeds the frame client width, expose a visible hint, compact boundary-aware controls, and a labeled focusable scroll region.
- The focused frame supports ArrowLeft, ArrowRight, Home, and End without intercepting keys from descendants.

## Browser Geometry

- Use `table.offsetWidth > frame.clientWidth` to decide whether table-level overflow exists. `table.scrollWidth` can include intrinsic overflow from clipped cell content even when the rendered table box fits.
- Use `frame.scrollWidth - frame.clientWidth` for the nominal horizontal range.
- With `scrollbar-gutter: stable`, Chromium's effective maximum `scrollLeft` may be smaller than that nominal range by the reserved gutter width. Apply `max(1, frame.offsetWidth - frame.clientWidth)` only to left/right boundary comparisons.
- Do not apply gutter tolerance to overflow existence; a one-pixel rendered overflow must still expose navigation.

## Verification

- `npm run build`, `npm run lint`, and `npm run typecheck`: pass.
- `npm test -- --run`: 20 files, 108 tests pass.
- `npx playwright test --config=playwright.task-overflow.config.ts --project=chromium --workers=1`: 1 focused production-preview scenario passes and regenerates six screenshots.
- `npx playwright test tests/e2e/tasks.spec.ts tests/e2e/task-19.spec.ts --project=chromium --workers=1`: 14 scenarios pass.
- Six captures at 1440x900, 1024x900, and 375x812 are valid RGB PNGs newer than rendered source.
- Two independent final visual reviews returned PASS with no blockers.

## Deterministic Right-Edge Intent (2026-09-05)

- Sticky right-edge behavior must use an intent ref independent of derived `canScrollRight`; a scroll event can observe enlarged geometry before a layout observer updates state.
- Process any leftward position change before geometry-growth anchoring. This lets even a sub-gutter movement cancel intent when the user scrolls left during the same render generation that expands the table.
- Explicit `End` navigation must establish intent directly, including when the effective range is no larger than the stable scrollbar gutter. Initial layout and repeated observer callbacks must never infer intent without navigation or rightward movement.
- Browser assertions should compare the captured position with the element's saturated `scrollLeft`, not assume `scrollWidth - clientWidth` is reachable when `scrollbar-gutter: stable` reserves space.
- Final verification: 110 Vitest tests, ESLint, TypeScript, production build, 30 consecutive focused Chromium scenarios, the complete 47-scenario Chromium suite, manual desktop/narrow interaction, and six responsive captures passed.

## Pre-growth End Intent (2026-09-05)

- A focused loading table can receive `End` before its asynchronous rows produce rendered overflow. Do not condition explicit keyboard intent on the current `hasOverflow` measurement.
- Model right-edge intent as `none`, `pending`, or `anchored`: pending survives initial no-overflow layout, anchored follows later geometry, and anchored clears when content overflow disappears.
- When the range contracts, compare left movement with the contracted edge. A browser clamp that follows the range delta preserves anchoring; additional left movement cancels it.
- Regression proof: the pre-growth component case failed at `0` versus expected `420` before correction. Final proof passed 112 Vitest tests twice, the exact 30-test Chromium reproduction, 60 uninterrupted focused Chromium executions, ESLint, TypeScript, production build, and all 47 Playwright scenarios.
