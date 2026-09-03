# Videnoa Controller Task 19

## Browser Regression Contract

- Playwright runs serially against the production preview and stores its HTML report, result metadata, and screenshots under `.omo/evidence/videnoa-controller/task-19/`. Trace and video capture stay disabled to avoid retaining authentication request material.
- The focused Task 19 suite covers serious/critical axe findings on login and all operational routes, worker-table keyboard overflow, edit-dialog containment and focus restoration, SSE reconnect/unavailable states, API 500 recovery, long CJK paths, reduced motion, forced colors, session expiry, and empty browser storage/cookies.
- Existing Task 15-18 scenarios remain the authoritative regression flows for authentication, task history and actions, workers, settings, errors, and responsive states. Their evidence paths are redirected into the Task 19 report tree rather than duplicated across stale directories.

## Accessibility And UI Corrections

- Light-theme quiet, accent, and healthy semantic tokens meet the route-level contrast scans without changing dark-theme hierarchy.
- Narrow worker tables expose visible Left/Right navigation, a named scroll region, and an associated hint. The edit dialog restores focus to its exact invoking row action.
- Reduced-motion overrides use sufficient specificity to suppress component transitions and iterations while preserving immediate state feedback.
- Worker form fields initialize synchronously on each mounted dialog session. This prevents a fast input from merging with stale controlled state while refreshed worker props still supply the latest optimistic version.

## Verification

- `npm run test -- --run`: 102 tests passed.
- `npm run typecheck`, `npm run lint`, and `npm run build`: passed.
- `npx playwright test tests/e2e/operations.spec.ts --project=chromium --workers=1 --repeat-each=5 --reporter=list`: 5 repetitions passed.
- `npx playwright test --project=chromium --workers=1 --reporter=list`: 38 scenarios passed.
- Final evidence contains exactly 62 fresh valid PNGs, `.last-run.json` records `passed`, and no error context, trace, video, cookie, browser-stored credential, private key, or usable token remains.
- Two independent final visual passes inspected all 62 captures and returned PASS with no product or evidence blockers.
