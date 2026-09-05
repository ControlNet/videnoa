# F3 Real Manual QA and Visual Verification

QA date: 2026-09-05

Runtime and product revision: `0fb4eb597acda9b571efc686c4701da333831675`

Repository tip at report completion: `0fb4eb597acda9b571efc686c4701da333831675`

## Verdict

**REJECT**

The exact-tip Controller passes production builds, all 31 Task 20 production-shaped tests, 112 Controller Web unit/component tests, 21 focused Chromium regressions, real release-binary authentication and persistence, restart recovery, CJK task intake and cancellation, worker onboarding and offline health, settings mutations, responsive overflow, keyboard/focus behavior, reduced motion, 20,000-row history, and logout failure recovery.

Release approval remains blocked because a valid Korean worker name is unreadable in the production Workers table. The authoritative DOM and SQLite row retain `렌더-노드-서부-매우긴이름`, but Chromium renders the name as replacement-like block/line glyphs at desktop, tablet, and mobile widths. Chinese/Japanese strings render correctly in the same runtime. An operator therefore cannot reliably identify this worker through the supported visual surface.

## Blocking Finding

### F3-B1: Workers table does not reliably render Hangul worker names

1. Registered `렌더-노드-서부-매우긴이름` through the real release UI alongside Latin and Chinese worker names.
2. Confirmed the exact Korean string persisted in SQLite and remained present in the browser DOM.
3. Observed unreadable replacement-like glyphs in `04-workers-cjk-offline-1440x900.png`, `05-workers-left-1024x900.png`, and `07-workers-left-375x812.png`.
4. Confirmed Chinese `渲染節點-東`, Japanese task paths, and mixed CJK task names render legibly, isolating the defect to incomplete font fallback rather than corrupt persisted data.
5. Audited the production CSS: the global stack is `Manrope, "Avenir Next", "Segoe UI", sans-serif`; explicit Korean/CJK fallback names exist only in task-detail CSS, not the Workers table/global application stack.

This is release-blocking for an operations UI that accepts Unicode worker identities: the stored value is correct but the primary visual identity is not readable. Provide a bundled or reliably available Hangul-capable fallback for operational surfaces and add a screenshot/browser regression using real Korean text.

## Environment and Method

- Verified `HEAD == origin/dev == 0fb4eb597acda9b571efc686c4701da333831675` before the final report update.
- Built the exact-tip production frontend and release Controller using the repository-required runtime environment.
- Ran the release Controller at `http://127.0.0.1:43189/` with isolated SQLite, NAS roots, temp state, log, output, and ephemeral authentication under `/tmp/opencode/videnoa-f3-final`.
- Used real Chromium at `1440x900`, `1024x900`, and `375x812` for login, errors, Tasks, task detail, dialogs, Workers, Settings, scrolling, keyboard navigation, reduced motion, and logout recovery.
- Used synthetic unreachable worker endpoints and a synthetic logout `503` only as explicitly test-only fault fixtures; no product implementation used placeholders.
- Correlated browser behavior with supported HTTP actions, the release Controller process, SQLite state, filesystem state, focused regressions, source audit, and 34 fresh screenshots.
- Closed Chromium, stopped the Controller and credential bridge, and removed `/tmp/opencode/videnoa-f3-final` after collecting sanitized evidence.

## Quality Gates

| Command or authority | Result |
|---|---|
| `npm run build` | PASS, production frontend bundle built |
| `cargo build --locked -p videnoa-controller --release` | PASS, release Controller built |
| `cargo test --locked -p videnoa-controller --test task20` | PASS, 31/31 including real HTTP pipelines, retry, outage, restart matrices, cancellation, and cleanup |
| `npm run lint` | PASS |
| `npm run typecheck` | PASS |
| `npm run test` | PASS, 20 files and 112 tests |
| Focused Chromium shell, Task 19, Task 21, and overflow suites | PASS, 21/21 |
| Exact-tip 20,000-task bounded backend load | PASS, 1/1 |
| Exact-tip Chromium 20,000-row task scenario | PASS, 1/1 |
| PNG signature/dimension validation | PASS, 34/34 valid non-empty PNGs at expected viewport dimensions |

The production build emitted only the known non-fatal Rollup annotation warnings from Zod v4 `core/regexes.js` and `core/util.js`.

## Functional Results

| Scenario | Result | Evidence observed |
|---|---|---|
| Wrong password | PASS | Login returned the expected failure and focused the visible error summary. |
| Successful login and storage | PASS | Shell loaded; local/session storage stayed empty and the HttpOnly session was absent from `document.cookie`. |
| Controller restart | PASS | Release process restarted, the existing session remained authenticated, and durable tasks/workers/settings reloaded. |
| Manual CJK task intake | PASS | Real HTTP `201` created task `b13c86fd-d8ec-4a1d-b27b-39343fc157d3` with long Japanese/Chinese paths. |
| Task filtering | PASS | Searching `劇場版` returned exactly one matching row and preserved URL query state. |
| Cancellation | PASS | Confirmation focus containment, supported cancel action, SSE/refetch convergence, authoritative Cancelled state, and trigger focus restoration passed. |
| Retry and fault recovery | PASS | Task 20 explicit processing retry and remote/local restart matrices passed in the exact-tip 31-test suite. |
| Worker onboarding/health | PASS functionally | Three workers persisted through supported UI actions; unreachable fixtures remained explicitly offline with retained health errors. |
| Worker identity rendering | **FAIL** | Korean worker identity persisted correctly but was unreadable in every captured Workers viewport. |
| Settings mutation | PASS | Concurrent uploads persisted as `2`; pause/resume persisted and Settings remained normally scrollable at all widths. |
| Task/worker overflow | PASS | Component-owned horizontal scrolling, controls, Home/End/arrow behavior, focus outline, and no document-level horizontal overflow passed. |
| Task detail | PASS | Long CJK paths wrapped, narrow history bottom was reachable, and close restored focus to the originating task row. |
| Reduced motion | PASS | Media query was active and visible transitions/animations collapsed to effectively zero duration. |
| Accessibility | PASS in automated scope | Targeted axe route/detail checks reported no serious violations; keyboard/focus regressions passed. |
| Logout failure/retry | PASS | Synthetic `503` preserved authentication, focused a fully visible alert, and retry reached login with password focus. |

## Durable and Filesystem Evidence

Before cleanup, isolated SQLite contained:

- Cancelled manual task `b13c86fd-d8ec-4a1d-b27b-39343fc157d3`, version `1`, zero attempts, zero retries, and no failure code.
- Queued API task `a8f0618d-4521-41c3-aabe-38688f8677db`, version `0`, zero attempts, and zero retries.
- Three enabled/offline workers with one, two, and three compute slots; each retained `worker health check failed` after the unreachable test endpoint probes.
- Settings version `3`, scheduler resumed, one default compute slot, one prefetch slot, two concurrent uploads, and one concurrent download.
- Two authentication-session rows total and one active session after logout and reauthentication.

The CJK source file existed under the isolated input root. The cancelled zero-attempt task produced no output and the temp/output roots remained empty. The release log contained only the expected warning that this isolated HTTP runtime did not require Secure cookies.

## Browser Diagnostics and Security

- The final console export contained one expected `503` resource error from the deliberately intercepted logout request and no unexpected warning.
- The session cookie was HttpOnly, SameSite Strict, and scoped to `/`; JavaScript storage exposed no authentication material.
- The report and evidence omit the ephemeral password, password hash contents, cookie value, CSRF proof, request headers, and authorization material.
- The temporary credential bridge was loopback-only, used only to transfer the ephemeral password into the automated browser without logging it, then stopped before cleanup.

## Visual Review

All 34 exact-tip captures under `.omo/evidence/videnoa-controller/final/F3-exact-tip/captures/` were validated and inspected. They cover login and error focus; empty, filtered, overflow, detail, cancellation, and CJK task states; worker onboarding, health, validation, and overflow; Settings running, paused, validation, top/bottom scrolling; logout failure; and all required viewports.

Positive observations:

- The former desktop Settings scroll-containment defect is corrected: `.shell-main` reaches bottom content while the sidebar/footer and Sign out remain visible at 1440 and 1024 pixels.
- The former narrow logout overlay defect is corrected: the focused alert occupies layout space and does not cover operational controls.
- Task and worker horizontal overflow remains component-owned, keyboard-operable, visibly focused, and recoverable at all tested widths.
- Japanese/Chinese filenames and paths render without mojibake or document widening; long values wrap inside their owning surfaces.
- Dialogs fit their viewports, destructive actions are distinguished, status/error states include text, and focus restoration passed.

Blocking observation:

- The Korean worker name is replaced by unreadable glyph shapes on the Workers route at every captured width despite correct DOM and database text.

Non-blocking observations:

- Long task paths wrap densely at 1024 pixels but remain readable and available through titles/detail.
- The compact Pause action stacks its icon and label; it remains fully operable and does not clip.
- Sticky mobile navigation naturally covers already-scrolled-off preceding text at intermediate scroll positions; all controls and content remain reachable.

## Independent Review

Two independent read-only reviewers audited the exact-tip artifacts and relevant Controller Web source:

- Reviewer A returned `REJECT`, High/P1. It independently confirmed that captures 04, 05, and 07 show an unreadable Korean worker identity, traced the value through `WorkerTable.tsx`, and identified the missing global/operational Korean font fallback. It passed Settings containment, narrow shell behavior, logout alert, focus, and overflow.
- Reviewer B returned `APPROVE` with residual Hangul risk, but stated that no Hangul-specific capture was found. That premise is contradicted by the authoritative SQLite/DOM value and the explicitly captured Korean row in 04, 05, and 07, so its approval does not clear F3-B1. It separately passed the shell, dialogs, reduced motion, accessibility, logout, and overflow implementation.

The direct artifact inspection and Reviewer A agree on the blocking symptom. Reviewer B's positive findings support the non-Hangul passes but do not negate the unreadable captured identity.

## Audit Boundaries

- No direct SQLite mutation created or changed the task, workers, settings, cancellation, login, logout, or restart result.
- Unreachable worker endpoints, high-volume generated rows, and the intercepted logout failure were test-only fixtures and are not presented as product implementations.
- No product source, test, lockfile, plan, or unrelated report was modified during F3.
- Pre-existing F1 and F2 report edits were preserved and excluded from the F3 commit scope.

VERDICT: REJECT
