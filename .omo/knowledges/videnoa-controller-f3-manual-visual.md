# Videnoa Controller F3 Manual and Visual QA

## 2026-09-05 Exact-Tip Revalidation

- Exact runtime and repository revision: `0fb4eb597acda9b571efc686c4701da333831675`; final report remains `.omo/evidence/videnoa-controller/final/F3-manual-visual.md`.
- The prior desktop Settings scroll-owner and narrow logout-alert overlay blockers are fixed. Real release-binary Chromium verified normal Settings bottom reachability with the sidebar/footer retained and a dedicated narrow alert row.
- Current verdict is `REJECT` because the valid Korean worker identity `렌더-노드-서부-매우긴이름` persists correctly in SQLite/DOM but renders as unreadable replacement-like glyphs in Workers captures at 1440, 1024, and 375 pixels.
- The global UI font stack lacks the explicit Korean/CJK fallback names used by task-detail CSS. Future remediation should provide a bundled or otherwise reliable Hangul-capable font and add a screenshot/browser regression with real Korean text.
- Exact-tip verification passed all 31 Task 20 tests, 112 Controller Web unit/component tests, 21 focused Chromium regressions, production builds, direct restart/session recovery, 20,000-row bounded load/browser scenarios, and all non-Hangul manual surfaces.
- Fresh captures are under `.omo/evidence/videnoa-controller/final/F3-exact-tip/captures/`; they are ignored evidence artifacts, while the tracked report records their conclusions.
- The isolated runtime and temporary password bridge were stopped and `/tmp/opencode/videnoa-f3-final` was removed after evidence collection.

Date: 2026-09-04

Runtime revision: `ca0b27e7f3bd07a394903504a39b35a759a8f321`

Final report: `.omo/evidence/videnoa-controller/final/F3-manual-visual.md`

## Durable Findings

- F3 verdict is `REJECT` because a worker registered through the production UI/API is created offline with empty capabilities and no production runtime refreshes it into a schedulable state. A compatible queued task therefore remains unassigned with no attempt.
- Direct SQLite worker health/capability injection was used only after reproducing the defect, to continue downstream QA. It must never be cited as successful worker onboarding.
- After that intervention, the full CJK pipeline, task intake idempotency, restart recovery, pause persistence, cancellation, 20,000-row pagination/query-plan behavior, temp cleanup, and remote cleanup passed.
- Desktop Tasks can overflow its 78-rem minimum-width table while explicit Left/Right controls are hidden above 48 rem. Narrow layouts expose the controls and keep overflow inside labeled regions.
- Shutdown timing was inconsistent: one observation remained alive beyond 32 seconds and needed force-stop; a controlled repeat exited after 29 seconds. Treat this as an intermittent contract risk pending an external-process regression, not the decisive F3 blocker.
- CJK glyph rendering and narrow page containment were good in the captured states. Axe on narrow Settings reported zero WCAG A/AA violations.

## Evidence Hygiene

- Runtime data is isolated under `/tmp/opencode/videnoa-f3-ca0b27e`.
- Authentication credentials, password hash, cookie, CSRF value, and authorization data are excluded from durable evidence.
- Controller and both test worker processes were stopped after QA.
- Current tip `b06567b6d9cc3ad0466dd56f04e2a3e5d60f6144` differs from the runtime revision only by the F1 evidence report, not product source.

## 2026-09-05 Shell Containment Remediation

- This remediation addresses two later shell-specific F3 defects and does not rewrite the historical 2026-09-04 verdict above.
- Desktop root cause: the implicit Grid row expanded to Settings intrinsic height because equal min/max block-size constraints did not establish a definite grid block size. Replacing them with `block-size: 100dvb` constrains the row and leaves `.shell-main` as the only vertical scroll owner.
- Narrow root cause: the fixed-bottom logout alert overlaid task controls. At the mobile breakpoint, `.app-frame:has(.shell-alert)` now allocates a dedicated alert row and moves main content to the following row.
- Regressions use a normal Playwright wheel gesture, shell/frame scroll invariants, Sign out and final-content viewport assertions, focus-outline containment, and enabled-control intersection checks.
- Fresh artifacts are under `.omo/evidence/videnoa-controller/task-19/f3-layout-fix/`: `settings-1440x900-scrolled.png`, `settings-1024x900-scrolled.png`, `logout-alert-375x812.png`, and `qa.md`.
- Deterministic browser fixtures are synthetic test data shaped like the production API. They contain no usable credentials or authorization material.
