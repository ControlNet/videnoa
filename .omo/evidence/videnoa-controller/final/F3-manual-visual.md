# F3 Real Manual QA and Visual Verification

QA date: 2026-09-05

Exact source tip under review: `30d9f25d19cf0ec1a88733483da7f95581e980ad`

Prior full visual approval revision: `e161e772744c48791f67cb21575c6ebef4ace13c`

Prior broad real-runtime evidence revision: `0fb4eb597acda9b571efc686c4701da333831675`

## Verdict

**APPROVE**

The Videnoa Controller passes fresh F3 manual authentication QA and visual verification for the authorized initial release scope of English and Chinese at exact source tip `30d9f25`. No in-scope product, authentication, visual, responsive-layout, accessibility, or evidence blocker remains.

Korean is explicitly out of scope. This report does not claim Korean support or claim that the prior Korean rendering observation is fixed.

## Source and CI Identity

| Revision | `controller-web` tree | `crates/controller` tree | Attribution |
|---|---|---|---|
| `0fb4eb5` | `1e21aa9ac5a1546ecafacf50473ef1c10afed070` | `49f0a2f356e0ca9eb36c446a5a89679c9d242ebc` | Prior broad release-runtime evidence |
| `e161e77` | `1e21aa9ac5a1546ecafacf50473ef1c10afed070` | `49f0a2f356e0ca9eb36c446a5a89679c9d242ebc` | Prior 31-capture English/Chinese visual approval |
| `30d9f25` | `1e21aa9ac5a1546ecafacf50473ef1c10afed070` | `0eb1068692b827c107ac5ba51692d38bed5ba03b` | Fresh authentication runtime and targeted visual recheck |

`controller-web` is byte-identical between `e161e77` and `30d9f25`. The exact-tip commit changes nine Controller Rust authentication/error/test files and moves password-file loading plus Argon2 verification to `spawn_blocking`; it does not change the frontend tree.

Read-only GitHub inspection verified hosted run `33946244764` at head SHA `30d9f25d19cf0ec1a88733483da7f95581e980ad`: status `completed`, conclusion `success`, all 14 jobs successful. The run includes Controller web lint/unit/build/Chromium, Controller Rust formatting/Clippy/tests, fault/load suites, Linux and Windows archives, images, legacy packages, and workflow contracts.

## Fresh Exact-Tip Runtime Method

- Rebuilt `target/release/videnoa-controller` from the exact source tip with `cargo build --locked -p videnoa-controller --release`.
- Started that release binary on loopback with isolated SQLite, temp, input, and output roots under `/tmp/opencode/videnoa-f3-30d9f25`.
- Used an ephemeral generated password and Argon2id hash. A loopback-only no-log credential bridge supplied the password to Playwright without placing it in commands, browser logs, report text, captures, or storage.
- Configured `secure_cookie = false` only for isolated loopback HTTP. The runtime emitted the expected trusted-HTTP warning and no other log entry.
- Drove real Chromium against the release runtime. Only the first logout response was deliberately intercepted as `503` to verify recoverable logout behavior; the retry reached the real Controller.
- Closed Chromium, stopped the bridge and Controller, and verified both ports and processes were gone.

## Fresh Authentication and Bearer Results

| Scenario | Result | Fresh exact-tip observation |
|---|---|---|
| Initial session bootstrap | PASS | Unauthenticated `/api/auth/session` returned `401` and the password field received focus. |
| Wrong password | PASS | Login returned `401`; the visible error summary read `The password was not accepted.` and owned focus. |
| Successful login | PASS | The authenticated shell loaded and the Tasks route rendered. |
| Concurrent Bearer verification | PASS | Eight simultaneous Bearer-authenticated `/api/readiness` requests all returned `200` with all readiness checks ready. |
| Bearer limiter | PASS | Six invalid Bearer requests returned `401, 401, 401, 401, 401, 429`. |
| Bearer limiter reset | PASS | A successful Bearer request returned `200`; the next invalid Bearer request returned `401`. |
| Authenticated cookie route | PASS | `/api/tasks?limit=25&offset=0` returned `200` with the empty isolated-runtime task page. |
| Logout failure preservation | PASS | Deliberate one-shot `503` kept the authenticated Settings shell mounted, displayed the retryable alert, and focused it. |
| Logout retry | PASS | The second Sign out reached the real Controller, returned `200`, removed the session cookie, and focused the login password field. |
| Reauthentication | PASS | Login succeeded again after logout and authenticated route navigation remained functional. |
| Final logout boundary | PASS | Cookie jar became empty and the protected Tasks API returned `401`. |

The session cookie metadata observed before logout was `HttpOnly`, `SameSite=Strict`, `Path=/`, with a 43-character opaque value that was not recorded. It was non-Secure only because this isolated runtime deliberately used loopback HTTP.

## Security and Diagnostic Results

- `localStorage` and `sessionStorage` remained empty before and after login/logout.
- `document.cookie` remained empty while authenticated because the session cookie was HttpOnly.
- The credential did not appear in rendered body text, input values after the authenticated shell mounted, browser storage, current-tip captures, Playwright artifacts, or the report.
- Browser console errors were limited to expected deliberately exercised HTTP outcomes: unauthenticated session `401`, wrong login `401`, and intercepted logout `503`. Chromium also emitted its informational password-form username-field suggestion.
- API request inventory exposed only method, URL, and status; no Authorization, cookie, CSRF, password, or hash value was exported.

## Visual Evidence Attribution

### Prior full coverage, source-identical frontend

All 31 PNGs under `.omo/evidence/videnoa-controller/final/F3-english-chinese-recheck/captures/` were directly re-read and inspected in this review. They were captured at `e161e77`, not newly captured at `30d9f25`, and remain valid visual evidence because both revisions use the exact `controller-web` tree `1e21aa9ac5a1546ecafacf50473ef1c10afed070`.

That 31-image set preserves complete desktop/tablet/mobile coverage for login; Tasks left/right, detail, and intake; Workers left/right and edit; Settings top/bottom; and logout failure at `1280x900`, `768x900`, and `375x812`. English and Chinese names, paths, labels, statuses, and errors remain readable without tofu, mojibake, clipping, baseline loss, or document-level horizontal overflow. Table overflow is intentionally component-owned and navigable.

The 34 captures under `.omo/evidence/videnoa-controller/final/F3-exact-tip/captures/` remain prior broad real-runtime evidence only. They are not represented as fresh `30d9f25` captures.

### Fresh `30d9f25` targeted captures

The report itself is the sanitized exact-tip manifest for these 8 ignored evidence PNGs under `.omo/evidence/videnoa-controller/final/F3-30d9f25-auth-recheck/captures/`:

| Capture | Dimensions | SHA-256 | Purpose |
|---|---:|---|---|
| `login-error-desktop-1280x900.png` | `1280x900` | `57c5b3c658dbc20c7413cd245a292a7af9cec5c9708fd9c83e098a9d45afc8fd` | Fresh wrong-password error and focus |
| `login-tablet-768x900.png` | `768x900` | `cdff8c16023bdbeea97f919480c66152063cfcf1e1af8e9d32a32daad1798ddb` | Fresh tablet login layout |
| `login-mobile-375x812.png` | `375x812` | `a473960184eb39a051404717b4962ff06b374fdeeb667c118836632c169fd2f1` | Fresh mobile login layout |
| `tasks-authenticated-tablet-768x900.png` | `768x900` | `31f693a5739d053c1251c8768266f7afb27187d61e1e35a33091b54510c3f75f` | Fresh authenticated route and tablet shell |
| `settings-bottom-desktop-1280x900.png` | `1280x900` | `007c8f22d6e49c26dc9526ce3dd90e174864ef1a9abb7628a3d889295a9e43f0` | Fresh Chinese roots and desktop Settings reachability |
| `settings-bottom-tablet-768x900.png` | `768x900` | `ac553a5b7c806fdc4a6dff8f97bc1d1b19266587e9a2351287590f298bd68c41` | Fresh Chinese roots and tablet Settings reachability |
| `settings-bottom-mobile-375x812.png` | `375x812` | `b2ae882ccb81d715417849088b1c704f11dc59937f8376908d8861bf86274b30` | Fresh Chinese roots and mobile Settings reachability |
| `logout-error-retry-mobile-375x812.png` | `375x812` | `384157dd12feb417a3b5eddb5933714c9d60c4978008e18b591da53c1d0a6cd3` | Fresh recoverable logout alert and focus |

All 8 files had valid PNG signatures and the stated dimensions. Browser measurements reported loaded fonts and no document-level horizontal overflow. The Settings DOM contained the Chinese input/output root names `输入` and `输出` at all three widths. Direct inspection found no English/Chinese glyph, wrapping, clipping, overlap, focus, dialog, shell, or reachability blocker.

## Prior Broad Runtime Evidence Preserved

The unchanged product areas do not need to be misrepresented as freshly rerun. Prior real release-runtime evidence at `0fb4eb5` remains authoritative for SQLite persistence, Controller restart and session recovery, task intake/cancellation, worker onboarding/health, Settings mutations, 20,000-row history, filesystem effects, and fault paths because those product trees match `e161e77`. Exact-tip hosted run `33946244764` supplies the current regression confirmation across those suites.

## Independent Review Status

- Fresh Chinese visual-precision reviewer: `PASS`, high confidence, all 39 required images enumerated and inspected, no product or evidence finding, no blocker.
- Fresh design-system/functional and evidence-attribution reviewer: `PASS`, high confidence, all 39 required images inspected, all 8 exact-tip filenames/dimensions/SHA-256 values verified, exact tip/tree identity and run `33946244764` verified, no blocker.

An earlier independent design-system pass returned evidence-only `REVISE` because the previous report still named `e161e77` as its completion tip and the 8 new captures lacked a sanitized exact-tip manifest. It found no product issue. This report corrected both points before the fresh final `PASS`; the stale verdict was not reused as approval.

## Audit Boundaries

- No product source, tests, lockfile, plan, F1, F2, or F4 report was modified during this F3 pass.
- No direct SQLite mutation was used to manufacture authentication or UI success.
- The one-shot logout `503` was a browser fault injection and is not presented as a real Controller failure.
- Prior fixture visual evidence and prior real-runtime evidence are explicitly separated from fresh `30d9f25` release-runtime observations.
- Korean support is neither approved nor rejected by this English/Chinese gate.

VERDICT: APPROVE
