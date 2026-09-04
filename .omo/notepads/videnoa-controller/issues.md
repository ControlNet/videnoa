# Issues

## 2026-09-02 Task 1

- Strict release Clippy exposed cfg-specific unused imports and a zero-sized release asset ownership mismatch. Both were resolved with cfg-scoped imports and a private `FrontendAssetSource` enum; no lint suppression was added.

## 2026-09-02 Task 1 Fix Round 1

- Independent verification found three blockers in `6f23c16`: Cargo 1.83 manifest parsing, SPA HTML leaking through unknown API paths, and dark accent contrast below WCAG AA. All three now have reproducible red evidence and focused green verification.

## 2026-09-02 Task 1 Fix Round 2

- Review Round 2 found that `/api` and `/api/` still escaped the wildcard boundary and that four Controller Rust files failed `cargo fmt --all -- --check`; both blockers now have red evidence and cross-profile green verification.
- The documented frontend E2E gate cannot run because `controller-web/package.json` has no `test:e2e` script; install, lint, Vitest, and production build all pass.

## 2026-09-02 Task 1 Fix Round 3

- Review Round 3 found encoded API paths reaching SPA HTML, release-only POST/OPTIONS HTML responses, and weak SPA body assertions. All now have cross-profile regression coverage and live verification.
- The first frontend test attempt found a corrupt local `node_modules/pathe` installation; `npm ci --no-fund` restored the lockfile-defined dependencies, after which lint, Vitest, and build passed.

## 2026-09-02 Task 1 Fix Round 4

- Review Round 4 found release encoded static assets falling through to SPA HTML and decoded backslash/control paths reaching fallback behavior. Typed decoded-path propagation and unsafe-character rejection now have debug/release regression and live coverage.

## 2026-09-02 Task 1 Fix Round 5

- Closure QA found dot and repeated-empty segments resolving to JavaScript through debug `ServeDir` but SPA HTML through release `rust-embed`. The decoded segment parser now rejects these spellings consistently, and asset assertions independently exclude SPA HTML.

## 2026-09-02 Task 1 Fix Round 6

- Final review found `ServeDir` normalizing a real asset's literal or encoded trailing separator in debug while release exact lookup returned SPA HTML. The debug fallback now consumes `DecodedPath` directly and has a dynamic cross-profile regression.

## 2026-09-02 Task 1 Fix Round 7
- Security review found raw `PathBuf::join` could interpret Windows prefixes or ADS syntax. Shared component eligibility now prevents those disk reads.

## 2026-09-02 Task 1 Fix Round 8

- Follow-up review found superscript COM/LPT aliases and `CONIN$`/`CONOUT$` absent from the exact-asset deny set; debug and release pure tests reproduced the superscript escape before the fix.
- The prior integration fixture constructed foreign Windows path spellings unconditionally; mutation-sensitive file creation is now compiled only for Unix debug tests.

## 2026-09-02 Task 2

- Figment's `Env` provider is feature-gated independently from TOML, so the workspace dependency must enable both `env` and `toml` explicitly.
- Strict Clippy required contract modules to split configuration validation and paging from their public type declarations; all Task 2 production and test files remain below the 250 pure-LOC ceiling without lint suppression.
- No Task 2 product issue remains after focused serialization, URL, paging, environment precedence, strict-schema, defaults, and filesystem-boundary tests passed.
- One broad QA attempt raced the release build script's `npm ci` against a standalone frontend `npm ci`; a clean serial rerun passed frontend lint, Vitest, TypeScript, Vite, and all release Rust gates.

## 2026-09-02 Task 5

- The initial live QA command used the binary name as the package name; the workspace package is `videnoa-app` with binary `videnoa`. The corrected command started the service and all HTTP probes passed.
- Full workspace formatting and Rust 1.83 verification were temporarily blocked by concurrent Task 3 files and dependency selection. Task 5 direct formatting, focused tests, current strict Clippy, core build, core regressions, LSP, and live API/database probes passed.

## 2026-09-02 Task 3

- Initial query-plan verification showed SQLite selecting status/worker indexes instead of the intended queue and recovery indexes; index column order and predicates were corrected and the 20,000-row EXPLAIN test now passes.
- Rust 1.83 cannot parse the newly resolved base64ct 1.8.3 manifest because Cargo 1.83 lacks edition-2024 support; the ignored workspace lockfile makes this a repository-wide dependency-resolution blocker rather than a Controller compile failure.

## 2026-09-02 Task 5 Follow-up

- Independent review found the initial handler consulted workflow files before the persisted idempotency mapping. Deleted/corrupt workflows reproduced 404/400 replay failures and masked collisions.
- Numeric review found `serde_json::Number::to_string()` preserved `1.0`, producing a different fingerprint from equivalent `1`.
- Numeric normalization must remain compatible with durable rows written before
  the change. On a stored-hash mismatch, canonicalizing the persisted workflow
  name and params snapshot bridges equivalent numeric spellings while preserving
  conflict detection for changed payloads.

## 2026-09-02 Task 3 MSRV Scope Correction

- Commits `b9c7e88` and `c4936ce` over-scoped the verified Controller MSRV to every workspace package. Rust 1.83 core reproduction fails before compilation while parsing exact `ort-sys 2.0.0-rc.12`.

## 2026-09-02 Task 4

- Final review found descendant directory checks followed by symlink-following opens and no rejection of symlinked root ancestors. The implementation now walks roots and descendants with no-follow descriptor opens, with dedicated regression tests.
- Follow-up review found that replacing a configured root left accepted paths attached to the old safe descriptor instead of invalidating queued work. Root identity is now revalidated before input reopen and output creation.
- The initial Task 4 change pushed `crates/controller/src/lib.rs` to 251 pure LOC. Moving `serve_authenticated` into `auth/http.rs` restored the plan ceiling.
- `cargo-audit` is unavailable in the environment. Secret Guard found only a pre-existing `token=` redaction-test literal outside Task 4 and reported pre-existing generic key-file `.gitignore` gaps.

## 2026-09-02 Task 6

- The first truncation implementation paired a fixed four-byte body with a larger `Content-Length`; Hyper panicked even though the client assertion passed. Replacing it with an erroring unknown-size stream removed the worker panic while preserving the body-read failure.

## 2026-09-02 Task 8

- Strict Clippy initially exposed the shared harness as public from the new integration target, promoting test helpers into undocumented public APIs. Keeping the harness private and compiling the existing harness scenarios into that target preserves dead-code coverage without lint suppression.
- The first Rust 1.83 run failed while parsing inactive `cpufeatures 0.3.1`; downgrading the reqwest lock graph's QUIC packages restored MSRV compatibility without changing the active HTTP/1 rustls feature tree.
- Separate top-level integration tests each compiled the full shared support module and produced strict dead-code failures. One test crate with focused submodules now compiles the reusable harness once.

## 2026-09-02 Task 8 Preset Contract Follow-up

- The original mock incorrectly allowed extensionless preset IDs to resolve through the saved-workflow interface endpoint, masking a production contract mismatch. Production-shaped slug and UUID fixtures reproduced both presets as incompatible before the fix.
- The mock and production clients now consume typed embedded preset interfaces, while real-TCP journal assertions prevent future synthetic preset interface requests.

## 2026-09-02 Task 9

- Rust 1.83 builds and all controller tests pass. An extra strict Clippy run on that older toolchain enables lint-version diagnostics across pre-existing modules; the required active-toolchain strict Clippy command passes cleanly.
- Follow-up review found that direct cancellation completion could close a submitting attempt without reconciling whether its keyed remote request was accepted. Typed accepted/not-accepted reconciliation now durably selects remote cancellation or staged cleanup before completion is allowed.

## 2026-09-02 Task 10

- Strict Clippy initially found an unnecessary test `Result` and an oversized fixture method. Both were corrected structurally without lint suppression.
- The first manual signal probe reached an unrelated process on an occupied port; rerunning on a unique port verified the intended controller process and durable pause behavior.

## 2026-09-02 Task 10 Contract Repair

- Review found startup-wide propagation for malformed rows, cancellation paths exposing ordinary continuation, unchecked remote identity, unbounded health retry counts, synthetic-only drain coverage, and a 358-line reconciler. All were reproduced and corrected without lint suppression.

## 2026-09-03 Task 11 Review Fix

- Independent review found that worker slot reduction checked durable usage before the optimistic worker update, allowing a reservation to commit between those operations and leave more assignments than compute slots. A checkpoint-controlled regression reproduced the invalid durable state, and the worker update now rejects it atomically with a typed capacity outcome.
- The originally generated evidence used generic filenames. Exact scheduler timeline and failure artifacts are now present under the Task 11 evidence directory.

## 2026-09-03 Task 12

- The first lifecycle SQL edit bound download evidence before the existing lifecycle timestamp placeholders, causing text values in integer timestamp columns. Reordering binds to match the statement fixed the corruption and all lifecycle/recovery regressions pass.
- The initial mismatch fixture used a 503 response fault that occurred after the mock accepted and stored the full upload, so exact stat correctly staged it. The failure scenario now uses the pre-accept disconnect fault and proves mismatched-partial deletion plus retry.
- The Task 12 integration target imports the complete shared real-TCP mock; a target-scoped `dead_code` expectation documents that intentional cross-target harness surface while strict Clippy remains clean.

## 2026-09-03 Task 12 Review Fix

- Independent review found seven gaps: retry SQL binding order, missing startup dispatch, non-atomic pause admission, upload restart PUT ordering, non-durable failure exits, rename-before-CAS recovery, and incomplete download evidence admission. Deterministic regressions now cover each gap.
- Strict Clippy exposed three 20 KB test arrays allocated on the stack; moving those fixtures to `Vec<u8>` kept the test intent while satisfying the all-targets gate.

## 2026-09-03 Task 12 Convergence Fix

- Second independent review found three remaining convergence defects: absent restart uploads looped through retry, persisted pause aborted startup dispatch, and verified artifacts were inspected only after network access. All three now have failing-first regressions and green production fixes.
- The documented inverse `cargo tree -p videnoa-controller -i videnoa-core` command reports no matching package because `videnoa-core` is outside the selected Controller dependency graph. Direct Controller tree inspection confirms the intended absence of core and GPU runtime dependencies.

## 2026-09-03 Task 12 Windows Durability Review

- Independent review found the remaining non-Unix branch returned `Unsupported` unconditionally, so every otherwise successful Windows download retried instead of advancing to `Verifying`. The policy regression reproduced that result before the cfg split.
- The installed MSVC Rust target is insufficient for a Linux-hosted dependency build: conda supplies GNU Linux `CC`/`AR`, while `clang-cl` and `llvm-lib` are absent. Native policy, lint, and full Controller tests pass; direct MSVC cross-check remains a host-tooling gap.

## 2026-09-03 Task 13

- Initial strict Clippy found an oversized cleanup function, manual let-else patterns, and an over-parameterized finalizer. The executor was split by publication, finalization, failure, and remote-cleanup responsibility with no lint suppression.
- Native Linux tests prove `RENAME_NOREPLACE`; direct Windows compilation remains unavailable on this host for the previously documented missing MSVC-compatible C compiler and archiver.

## 2026-09-03 Task 13 Review Fix

- Independent review rejected the original Task 13 evidence because ambient finalization escaped the capability boundary, non-regular artifacts could be opened, matching final plus staging was accepted, malformed cleanup aborted startup dispatch, tests overstated race/crash coverage, and `lifecycle_transition.rs` exceeded 250 pure LOC.
- The corrected focused suite has 14 Task 13 integration cases plus a direct created-parent replacement unit regression. The evidence no longer claims forced EXDEV, permission denial, FIFO, cancellation, or every crash window as executed scenarios.

## 2026-09-03 Task 13 Final Convergence

- Oracle's second review retained staging identity, ambient parent sync, metadata-before-open, production QA, and `paths/mod.rs` size blockers. Final rehash, retained-descriptor sync, opened-handle classification, 24 production-path scenarios, and a focused output-module extraction resolve them.
- Rust 1.83 tests and build pass with two pre-existing dead-code warnings in `scheduler/download_artifact.rs`; current strict Clippy is clean.

## 2026-09-03 Task 14

- The complete router previously exposed only authentication and task routes, so Task 14 required changing every full-router fixture and the production composition root to supply operational dependencies.
- Processing retry initially lacked a coordinator for the lifecycle contract's remote-terminal and workspace-cleanup evidence. The endpoint now obtains both from the assigned Videnoa worker before creating a fresh attempt.
- Initial SSE publication covered HTTP mutations only and reset authentication timing after every event. A store-scoped durable-change observer plus passive interval revalidation closes both gaps.
- Final review found processing retry deleted job history instead of the remote task workspace and returned the old attempt ID. The route now deletes `/api/files/{task_id}` before creating cleanup evidence and returns the newly committed attempt identity.
- Recovery worker-health and shutdown settings writes bypassed notifying service boundaries. Both now use `WorkerRegistry` or `Scheduler`, and scheduler runtime settings are validated before the durable CAS.

## 2026-09-03 Task 12 Input Identity Regression

- Same-size remove-and-recreate could reuse the exact device, inode, length, and nanosecond mtime, so metadata-only upload admission staged changed bytes and issued one PUT. New tasks now persist a separate truncated SHA-256 content identity while rooted reopen still checks platform metadata before and after hashing; migrated legacy rows retain nullable metadata-only admission.
- Rust 1.83 tests pass, but strict Clippy on that older toolchain remains blocked by 140+ pre-existing pedantic diagnostics in untouched modules. Current-toolchain strict Clippy passes.

## 2026-09-03 Task 16

- Visual review found toolbar overflow, inaccessible search focus, missing column scopes, and a skeleton persisting after initial load failure. The responsive grid, focus-within treatment, scoped headers, and explicit failed-load state resolve those defects.
- Rollup emits informational warnings while bundling two Zod annotation comments; frontend build and Playwright output remain successful.

## 2026-09-03 Task 16 Remediation

- Independent review blockers reproduced before fixes: deep empty offsets stepped backward one page per request, failed changed-query loads retained old rows, stale/equal SSE versions refetched, priority changes merged in place, filter metadata/live announcements were incomplete, and the original evidence did not prove coherent filtering or narrow rows/pagination.
- A new pagination-in-viewport browser assertion proved an additional evidence-rooted product defect: full desktop pages made the pagination footer unreachable inside the fixed shell. The table height budget now leaves the footer visible while retaining table-owned vertical and horizontal scrolling.
- The first regenerated evidence pass exposed duplicate `.png.png` captures beside stale `.png` files. The path helper was corrected, all seven affected Task 16 images were removed, and exactly five fresh correctly named PNGs were regenerated.
- The programming no-excuse helper could not resolve the project-local `typescript` package when launched through Bun. Project TypeScript, ESLint, Vitest, production build, and Playwright gates all ran successfully through the repository's npm scripts.
- Final independent visual review found two remaining narrow-table blockers: the diagnostic capture clipped the `Error` header, and horizontal overflow had no keyboard/assistive or visible navigation affordance.
- A focusable generic `div`, then a focusable semantic `section`, satisfied browser behavior but failed Biome's `noNoninteractiveTabindex` rule. The shipped resolution is a named non-focusable region with explicit native Left/Right buttons and a visible narrow hint.
- The `Error` column is wider than the narrow frame, so `scrollLeft = scrollWidth` and `offsetLeft` alignment both clipped its leading label. Viewport-delta alignment plus an 18 rem narrow long-cell bound now keeps `ERROR`, ellipses, rows, and pagination visible together.
- Two independent post-fix visual reviews returned PASS: exactly five fresh valid captures, visible narrow navigation and Error context, contained ellipses, readable pagination, and no remaining product or evidence blockers.

## 2026-09-03 Task 16 Final Remount

- Final functional review found that `useTasksData` initialized its applied update generation to zero, so Tasks -> Workers -> Tasks replayed the retained global update during loading and produced a second unnecessary page/count request pair.
- The failing browser regression observed three total task requests and three total count requests instead of two. Basing the mount ref on `appTaskUpdateStore.snapshot().generation` restores exactly one request pair on remount while existing newer-update merge and refetch scenarios remain green.
- Contradictory empty metadata rendered `10,001-123 of 123`. Range derivation now requires coherent non-empty page metadata and displays `0-0 of 123` without enabling forward pagination or entering a correction loop.
- Independent final functional and visual/integrity reviews both returned PASS with no remaining Task 16 blockers.

## 2026-09-03 Task 14 Oracle Blockers

- Processing retry accepted terminal jobs with contradictory or absent identity fields, then deleted the workspace and created a replacement attempt. It now reuses the recovery identity predicate before any cleanup.
- Status counts exposed sparse SQL groups directly. The API now returns all fourteen statuses in deterministic lifecycle order, including zero counts.
- Cancel/retry handlers duplicated lifecycle SSE publication and reloaded the task after commit, allowing publication/read failure to turn durable success into HTTP 500. The redundant handler path was removed.

## 2026-09-03 Task 15

- Initial Playwright bootstrap failed before issuing `/api/auth/session` because the injected native fetch received `ClientOptions` as its receiver. The receiver regression is now covered and all browser scenarios pass.
- Narrow layout hid the Sign out text and left an unnamed icon-only button. A state-aware accessible label restored keyboard and locator access without changing the visual layout.
- The first independent visual review found two acceptance blockers: connection lifecycle text disappeared below 48rem, and an extra stale focus capture showed Settings as `TASK 18`. A failing narrow Playwright regression now locks status visibility, narrow CSS hides only `/api/events`, and the stale capture was removed.
- No unresolved Task 15 product or evidence blocker remains. The final 14-file evidence set passed both independent reviewers. Production builds retain non-fatal Rollup warnings from Zod dependency comments.
- Ownership correction: the earlier interpretation that Settings belonged to Task 17 was incorrect. The plan assigns both Workers and Settings to Task 18; shell source, Vitest, Playwright, design documentation, durable knowledge, and all five Settings-bearing captures now agree.

## 2026-09-03 Task 15 Visual Remediation

- Follow-up review found four rendered gradients, an undeclared readiness font size, stale route captures, oversized narrow full-page images, ambiguous alert-focus evidence, and generated `test-results` residue.
- A failing browser contract reproduced the visual violations. Solid tokenized surfaces, viewport-only reduced-motion capture, explicit alert focus styling, and relocated Playwright output resolve the confirmed product and evidence blockers.
- The fresh inventory contains exactly 14 correctly sized PNGs and no `controller-web/test-results` files.
- Two fresh independent full-set reviews returned PASS with no product or evidence blockers.

## 2026-09-03 Task 15 Focus Convergence

- Independent full-suite verification reproduced the existing-session focus assertion intermittently with the Settings shell committed and `body` still focused. Moving route focus from a passive effect to a layout effect removed the scheduling window; the focused test passed 20 consecutive runs and Chromium reload coverage passed.

## 2026-09-03 Task 17

- The first full Playwright run found the Task 16 workflow filter selector ambiguous after the closed creation dialog added a second labelled Workflow input. Scoping the existing assertion to the `Task filters` fieldset preserves both accessible labels.
- The programming no-excuse helper still cannot resolve project-local TypeScript when launched from its external skill path through Bun. Repository TypeScript, ESLint, Vitest, build, and Playwright gates remain the authoritative project checks.
- Production builds retain the known non-fatal Rollup warnings for Zod annotation comments.
- Independent visual review found the quiet-text token below AA on light page canvas and dark elevated surfaces. Setting light/dark OKLCH lightness to `0.54`/`0.61` restores contrast while preserving the graphite hierarchy.

## 2026-09-03 Task 17 Oracle Remediation

- Functional review confirmed repeated cancellation visibility, retry eligibility broader than Rust, generic-only intake guidance, a non-UUID submission key, an open field-error code schema, and incomplete 409 request assertions. Each defect was reproduced by a focused failing test before correction.
- Visual review confirmed destructive initial focus, missing confirmation focus containment and nested Escape behavior, incomplete detail evidence, and absent executed no-clobber/late-cancel transcript entries. The fresh keyboard scenarios and exact eight-image inventory now cover those states without duplicate captures.
- The first complete-detail element capture was invalid because ancestor overflow clipped content and produced blank regions. It was removed and replaced by real top/lower scroll-state viewport captures.

## 2026-09-03 Task 17 Evidence Convergence

- The replay success fixture inherited priority 20,000 while the submitted body used 17. The fixture and field-specific detail assertions now align all submitted values, and the regenerated success capture visibly shows Priority 17.

## 2026-09-03 Task 18

- Initial shell and visual Playwright coverage still expected Task 18 placeholders; both suites now use operational worker/settings read fixtures.
- Visual QA found action focus leaving the worker table horizontally shifted, incomplete section coverage, stale narrow navigation timing, and edge-clipped readiness values. Explicit scroll positions, settled route assertions, full-section captures, and a local readiness inset resolved the evidence and product defects.
- Production builds retain the known non-fatal Rollup warnings for Zod dependency annotation comments.
- Functional review traced referenced-worker and capacity guidance through both the registry enum and the public operations translation. The final tests and fixtures lock the serialized HTTP strings from `operations/error.rs`, including empty conflict field-error arrays, API-URL duplication, capacity rejection, and referenced deletion.
- Final coverage review required browser-level stale worker/settings refetch and validation-bound evidence. The new stale-worker scenario exposed the open dialog reusing its original version; `WorkersPage` now supplies the refreshed record version while preserving local fields.

## 2026-09-03 Task 18 Remediation

- The first Chromium confirm-path run found focus restoration occurring before the rejected delete mutation completed; the disabled row action then lost focus. Restoration now waits for mutation settlement, while Escape and `Keep Worker` remain immediate.
- The deterministic Task 18 browser fixture previously modeled duplicate-name and unauthorized responses but did not execute both operator outcomes. The scenario and transcript now prove adjacent duplicate-name guidance and safe sign-in fallback after a `401` mutation.
- Independent visual review found the table overflow owner lacked a keyboard focus stop and the evidence packet omitted a post-`401` capture plus a complete runtime-editor capture. The region is now focusable and all three outcomes are covered by fresh browser evidence.

## 2026-09-03 Task 18 Accessibility Convergence

- Review found successful deletion targeting a detached row button, worker/settings errors without programmatic associations or first-invalid focus, and nine worker headers without explicit column scope.
- The first focused red run produced five intended failures. A subsequent real Chromium run found rejected-delete focus still raced the disabled mutation state; render-synchronized focus routing fixed the browser-only timing defect.
- The external Bun no-excuse helper still cannot resolve the project-local TypeScript package from its skill-cache path. Project-local ESLint, TypeScript, Vitest, build, and Playwright gates remain clean.
- Final visual review exposed missing server `compute_slots` mapping and incomplete state captures. The mapping, focused regression, validation/delete/table-focus captures, and fixed-size viewport captures were added before final gates.

## 2026-09-03 Task 19

- A full Chromium verification initially failed the Task 18 capacity-conflict branch because the edit dialog submitted `compute_slots=41` instead of `1`. The isolated scenario reproduced the failure; synchronous mount-time field initialization fixed it, and five repeated isolated runs plus the complete suite passed.
- React Doctor reports 56 remaining warnings in pre-existing schema, formatting, complexity, and analyzer-false-positive areas. It no longer reports the worker form state-in-effect defect; project TypeScript, ESLint, Vitest, build, and Playwright gates are authoritative and clean.
- Secret Guard reports two unchanged tracked fixture literals and 19 repository-wide `.gitignore` hardening opportunities. Task 19 evidence contains only an explicitly synthetic Playwright password string embedded in HTML test source and no usable credential, token, private key, cookie, trace, or video.

## 2026-09-03 Task 19 Session Remediation

- Acceptance review found that unauthorized session checks returned the correct typed `401` but left an invalid browser cookie intact, while the expiry scenario asserted an empty cookie jar without first creating that cookie. A real handler regression and a seeded browser scenario now cover both sides of the contract.
- The configured Chromium run passed all 38 scenarios and regenerated passing metadata. The final audit found 62 valid fresh PNGs, no identical content hashes, no stale failure markdown, and no trace, video, error-context, or cookie artifacts.

## 2026-09-04 Task 23

- Neither `pwsh` nor Windows PowerShell is installed on the Linux host, and the existing MSVC-compatible compiler/archive tooling gap remains. The Windows packager received static contract/layout review but no native PowerShell parser, Windows build, or executable smoke; evidence does not overclaim those surfaces.
- The first forbidden-content fixture used a non-contract archive filename, so filename validation correctly failed before member validation. Renaming the injected archive to the locked filename exposed and passed the intended exact-member rejection.

## 2026-09-04 Task 22

- The first live container start correctly rejected a mode-0600 root-owned bind-mounted hash file because numeric UID 10001 could not read it. The smoke fixture now uses a read-only bind mount with host permissions that permit the non-root process to read the PHC hash.
- The first unwritable-data assertion expected SQLite's `readonly database` wording, while the actual create/open boundary reports `unable to open database file`; evidence now pins the observed stable failure without exposing paths or credentials.
- No Task 22 product blocker remains after cached build, content/linkage scan, health, persistence, non-root write, missing-config/hash, read-only-data, outside-root, Rust 1.83, strict Clippy, and full Controller test verification.

## 2026-09-04 04:33:03 +10:00 Task 24

- The default Playwright port 4173 was occupied by an unrelated preserved process. The same 43-scenario production-preview suite passed through a temporary config on port 4193; no repository config or existing process was changed.
- Native Windows packaging and real GitHub/Docker Hub publication cannot execute on this Linux worktree. Windows-hosted wiring, exact artifact paths, action dependencies, and reusable static/package contracts are validated without overclaiming hosted execution.

## 2026-09-04 Task 25

- Native Windows command execution is unavailable on this Linux host. The docs
  use source-backed PowerShell commands, and the Windows archive/package runtime
  proof remains assigned to `windows-latest`.
- Real GitHub Release and Docker Hub publication weren't run locally. Exact
  image tags, archive names, dependencies, and publishing steps were validated
  against the tracked release workflow and packaging scripts.

## 2026-09-04 F4-B3 Legacy Linux Package Remediation

- Run `33799944410` failed only after p7zip scanned the complete 5,335,320,003-byte bundle and opened the archive path, then emitted opaque `System ERROR: E_FAIL` with exit 2. The same command passed on the immediately preceding runner image, identifying resource headroom rather than a deterministic command or layout defect.
- No hosted rerun was triggered for the working-tree remediation. Local real-bundle archive creation/integrity/layout, focused negative contracts, workflow contracts, and diagnostics are the available evidence for Atlas before landing.

## 2026-09-04 F4-B2 Auth Focus Remediation

- The hosted malformed-login focus failure was scheduling-sensitive: the exact test passed locally before the fix, but passive effect timing allowed committed error UI while focus remained on the password field.
- Concurrent task-table work introduced a deterministic jsdom-only `ResizeObserver is not defined` failure in full-shell auth tests. A test-local observer stub resolved it without touching the unrelated task implementation.
- The external programming-skill TypeScript audit helper still cannot resolve the project-local `typescript` package from its own caller path. Repository-native lint, typecheck, Vitest, build, diagnostics, and Chromium gates pass.
- Production builds retain the known non-fatal Rollup warnings for annotation comments inside Zod.

## 2026-09-04 07:10:53 +10:00 F1-B1/F3-B1 Worker Health Remediation

- Removing Task 20's direct `WorkerHealthUpdate` fixture exposed the production defect: an API-created worker remained offline with no attempts or remote run requests because no runtime owned general health refresh.
- The first full-suite rerun exposed a test-only race: an outage scenario used the normal registration helper, which now correctly waits for successful onboarding. A no-wait API helper now preserves the intended initial-failure observation.
- No worker-health product blocker remains. Current strict Clippy, current-toolchain tests, Rust 1.83 tests, formatting, diagnostics, and production-shaped HTTP scheduling coverage pass.

## 2026-09-04 F3-B2 Task Overflow Remediation

- Independent review found the initial measured-overflow state reused scrollbar-gutter edge tolerance for `hasOverflow`, which could hide a small real overflow. A one-pixel unit regression and rendered-table measurement now lock the corrected contract.
- The first two browser reruns exposed Chromium's effective scroll end and intrinsic cell-overflow distinctions. Runtime geometry confirmed the gutter mechanism; the final implementation uses table `offsetWidth` for affordance visibility and frame scroll geometry for boundaries.
- Final repository-native gates pass: production build, ESLint, TypeScript, 108 Vitest tests, the focused production-preview overflow scenario, and 14 existing Tasks/Task 19 Chromium scenarios. Production builds retain the known non-fatal Rollup warnings for Zod dependency comments.
- Two independent reviewers passed all six final fresh PNG captures with no product or evidence blockers.

## 2026-09-04 Rust 1.83 Clippy Compatibility

- The initial Rust 1.83 strict Clippy inventory contained 143 diagnostics: 95 `module_name_repetitions` and 48 `must_use_candidate`. After those were resolved, five masked `let_and_return` diagnostics appeared in `http_contract.rs` and were simplified without behavior changes.
- Full verification exposed inherited test fixtures that configured nonexistent `data_root` and `temp_root` directories before `PathCapabilities::open`; the fixtures now establish the same precondition required in production.
- One combined focused run observed a Task 20 cleanup assertion failure. The exact test, the full Task 20 target, and the final all-targets suite passed on quiet reruns, so no production cleanup change was justified.

## 2026-09-04 Rust 1.83 Clippy Compatibility Correction

- The prior remediation was rejected because `module_name_repetitions = "allow"` weakened package lint policy and `task_api.rs` exceeded the 250 pure-LOC ceiling.
- Removing the allowance reproduced exactly 95 `module_name_repetitions` diagnostics. Responsibility-based logical module names reduced the production inventory to zero without renaming serialized types or adding lint exceptions; equivalent test-support module cleanup removed the remaining test-target diagnostics.
- The first task API extraction placed support at `tests/task_api_support.rs`, which Cargo compiled as an unintended integration target. Moving it to `tests/task_api/support.rs` restored the intended private fixture boundary.

## 2026-09-04 Rust 1.83 Clippy LOC Completion

- Independent complete auditing found two touched test helpers above the hard ceiling: `mock_videnoa/recovery_support.rs` at 323 pure LOC and `task12/support.rs` at 360. The final NUL-safe audit also identified `src/main.rs` at 251.
- The first Task 12 utility re-export passed Task 12 but failed strict all-target Clippy because aggregate targets did not consume every re-export. Root delegate functions replaced the re-export without changing any caller path.
- No size blocker remains: the complete changed Controller Rust audit now has a maximum of 244 pure LOC.

## 2026-09-04 Final Wave F2 Task Overflow Determinism

- F2 observed native `disabled=true` followed by computed opacity `1` in one isolated repetition. Runtime frame sampling confirmed late layout growth re-enabled the stable right button before the split style read.
- The deterministic browser and component regressions failed before correction and now pass. No unresolved F2 product or evidence blocker remains after 30/30 focused Playwright executions and one exact 47/47 Chromium suite.
- Existing non-fatal Zod Rollup annotation warnings remain unchanged and are not related to the overflow correction.

## 2026-09-04 F3 SSE Shutdown Remediation

- Before the fix, an authenticated idle `/api/events` client kept the Controller process alive after the listener closed and beyond the documented 30-second shutdown bound. The release-process regression now exits in 11 ms without client disconnect.
- Strict Rust 1.83 Clippy is currently blocked by an `unnested_or_patterns` diagnostic in concurrently added `persistence/submission_claim.rs`; the corrected F3 files produce no Clippy diagnostic.
- The full all-target run is currently blocked by concurrently modified Task 20 pause, retry, and transfer-outage scenarios. Focused Task 20 shutdown and worker-health shutdown tests pass, and F3 did not modify orchestration, submission, persistence, or Task 20 files.

## 2026-09-04 F4 Duplicate Submission Remediation

- The original F4 failure was a same-generation recovery replay after uncertain remote acceptance. Both requests carried the same durable key and created one remote job, but violated the exact one-request-per-attempt requirement.
- Initial ownership placement before scheduler admission caused paused submissions to defer permanently in-process; moving the claim to the final pre-request boundary restored pause/resume behavior.
- The first restart-outage test paused the worker before journal recording, so aborting the client could cancel the first journal write. Synchronizing on the accepted request's journal entry removed that fixture race without sleeps or retries.
- Final audit found cancellation used the ownership claim but lacked an exact same-generation `/api/run` count assertion, and the migration test did not represent a pre-0006 database. Focused regressions now cover both contracts.
- The final F4 LOC audit found `task20/outage_matrix.rs` at 268 and `task20/support/controller.rs` at 253 pure lines. Wait helpers and reconciler fixture construction were split by responsibility; all F4-owned files now remain at or below 250.

## 2026-09-04 F3 SSE Hosted Startup Remediation

- GitHub job `100909271972` launched the debug Controller from a clean checkout without building ignored `controller-web/dist`. Startup returned `Controller frontend directory is missing` before listener bind, but the process test redirected stderr to null and exposed only exit status 1.
- The Controller Rust job now installs and builds Controller web assets before Cargo gates, with a workflow mutation contract that rejects removal of that build. The process test now reports early child stderr and has a real-binary missing-config regression proving the diagnostic remains visible.
- A forced competing listener produced a different wrong-listener/login-hang signature, not the hosted early-exit signature. The fixture's reserve/drop port pattern remains a separate residual harness risk requiring an inherited-listener or bound-address publication design, not retries or timing changes.

## 2026-09-04 Task 20 Hosted Contention Remediation

- The unchanged overlapping CPU-0 baseline failed both processes: one reported 26/30 and the other 27/30, reproducing bare download-part `NotFound`, opaque completion timeout, and additional scheduler-starvation failures.
- An in-process fixture budget of one removed the bare partial-file failure but did not pass the cross-process proof; both processes still stalled in `Submitting`. Adding the Task-20-only Unix advisory permit toggled the same proof to 30/30 in both processes without timeout increases or retries.
- No unresolved Task 20 infrastructure blocker remains after the final 30/30 target, overlapping CPU-0 proof, complete Controller all-target/all-feature suite, formatting, Rust 1.83 strict Clippy, and changed-file diagnostics passed.

## 2026-09-04 Task 20 AttemptMismatch Correction

- The prior statement that no Task 20 infrastructure blocker remained was premature. Independent verification produced process A 30/30 and process B 29/30, with `verifying_cancellation_removes_verified_workspace` returning `AttemptMismatch`.
- Operation-rich red evidence identified `finish checkpointed download`, not `request_cancellation`, as the failing call. Its inputs were task version 7/status `Downloading` and attempt version 8/status `Verifying` after the checkpoint was released before separate snapshot reads completed.
- The correction retains the Task-20-only admission guard and all cancellation assertions. It adds coherent snapshot acquisition only, holds the named checkpoint through cancellation-intent persistence, and reports operation, task/attempt IDs, versions, and statuses on lifecycle errors.
- Corrected verification passed 12 consecutive focused CPU-0 runs, then a fresh final-source overlapping proof at 30/30 in both processes, normal Task 20 at 30/30, the complete Controller all-target/all-feature suite, formatting, Rust 1.83 strict Clippy, and changed-file LSP diagnostics.

## 2026-09-04 Task 20 Remote Cleanup Crash Correction

- Independent overlapping verification exposed a remaining `RemoteDeleteSucceeded` failure: durable completion sometimes occurred with one `DeleteFile` request when the matrix expected recovery to replay deletion.
- Bounded diagnostics proved this was not a late mock counter or journal observation. After ten seconds, task and attempt were durably version 11/`Completed`, remote files were absent, and the delete counter remained one.
- The local fault matrix released its checkpoint after `ControllerRuntime::crash`. Because nested stage cancellation had no joined completion barrier, the release could wake the old cleanup future and allow it to commit completion before restart recovery ran.
- The correction removes only that post-crash release. Verification passed 10/10 focused CPU-0 matrices, two simultaneous CPU-0 Task 20 targets at 30/30 each, the complete Controller all-target/all-feature test suite, formatting, Rust 1.83 strict Clippy, no-excuse checks, `git diff --check`, and zero diagnostics across all 24 Task 20 Rust files.

## 2026-09-05 F2-B2 rustls-webpki Remediation

- Full `cargo deny check` no longer reports `RUSTSEC-2026-0049`, `RUSTSEC-2026-0098`, `RUSTSEC-2026-0099`, or `RUSTSEC-2026-0104`, and the Controller production tree contains no `rustls-webpki 0.103.9`. The command still fails on pre-existing or independently scoped workspace advisories and license policy findings.
- `RUSTSEC-2026-0190` for `anyhow 1.0.101` is reachable directly from the Controller normal/build graph and remains outside this exact webpki update. The reported `h2`, `time`, XML, GTK, and legacy Unicode findings are absent from the Controller normal/build tree or belong to other workspace products.
- The first Rust 1.83 all-target run overlapped the independently owned authentication limiter remediation and observed its temporary `401` versus expected `429` state. After those disjoint changes settled, the isolated limiter test and the complete Controller all-target/all-feature suite passed; no authentication files were edited by F2-B2.
- `cargo-audit` is not installed in this environment, so advisory evidence comes from `cargo deny check`, the inverse dependency tree, and exact advisory-ID absence checks.

## 2026-09-05 F1-B1 Task Overflow Determinism Completion

- A first manual assertion incorrectly treated `scrollWidth - clientWidth` as Chromium's reachable right edge and reported a ten-pixel gap. Runtime saturation confirmed that the gap was the existing stable scrollbar gutter, and the E2E assertion now compares against the browser's effective edge.
- Independent behavior review found missing coverage for initial non-anchoring, sub-tolerance left movement during growth, and explicit End at a gutter-sized range. Deterministic regressions now cover all three and pass.
- No TaskTable overflow blocker remains. Production builds retain the unchanged non-fatal Rollup warnings for annotation comments in Zod.

## 2026-09-05 F2-B1 Bearer Limiter Remediation

- Bearer password verification bypassed the existing peer-IP login failure budget because the shared authentication boundary did not receive the connection peer address. A failing real-router regression reproduced the sixth combined login/Bearer failure as `401` instead of typed `429`.
- Every active and passive Bearer boundary now receives the direct `ConnectInfo<SocketAddr>` peer IP. Task and operations middleware preserve the typed `rate_limited` response, while cookie-session validation remains outside the password failure budget.
- Live TCP verification changed `X-Forwarded-For` on every request and still observed five `401` responses followed by `429`; successful Bearer authentication cleared the direct peer state and the next invalid request returned `401`.

## 2026-09-05 F2-B2 anyhow Remediation

- The earlier F2-B2 report that Controller-reachable `RUSTSEC-2026-0190` remained is now corrected: the lock resolves `anyhow 1.0.103`, and the post-update deny report contains no occurrence of that advisory.
- Full `cargo deny check` still exits nonzero on independently scoped workspace policy findings: `RUSTSEC-2026-0258`, `RUSTSEC-2026-0194`, `RUSTSEC-2026-0195`, `RUSTSEC-2026-0009`, unmaintained GTK/desktop dependencies, and existing license rejections. No advisory ignore or policy downgrade was added.
- The package-scoped Controller normal/build tree excludes `h2 0.4.13`, `quick-xml 0.38.4`, and `time 0.3.45`; the workspace-wide deny graph can still display shared metadata paths through packages selected elsewhere in the workspace.

## 2026-09-05 F2-B2 h2 Remediation

- This entry supersedes the earlier statements that `h2 0.4.13` was outside Controller reachability. Independent deny evidence and the combined workspace inverse tree prove Controller paths through Axum/Hyper and Reqwest/Hyper-Rustls.
- The lock now resolves `h2 0.4.16`, and post-update `cargo deny check` contains no `RUSTSEC-2026-0258`, `RUSTSEC-2026-0190`, or prior webpki advisory ID. No ignore, allowlist, manifest pin, or deny-policy change was added.
- The remaining vulnerability advisories are `RUSTSEC-2026-0194` and `RUSTSEC-2026-0195` through desktop `quick-xml`, plus `RUSTSEC-2026-0009` through desktop/core `time`; none reaches Controller. Remaining unmaintained advisories (`RUSTSEC-2024-0370`, `RUSTSEC-2024-0411` through `RUSTSEC-2024-0420`, `RUSTSEC-2025-0057`, `RUSTSEC-2025-0075`, `RUSTSEC-2025-0080`, `RUSTSEC-2025-0081`, `RUSTSEC-2025-0098`, and `RUSTSEC-2025-0100`) are desktop-only.
- Full deny status remains `advisories FAILED, bans ok, licenses FAILED, sources ok` because those independently scoped workspace advisories and existing license rejections remain.

## 2026-09-05 Controller Logout Focus Ownership

- The first required `task-overflow.spec.ts --repeat-each=5` browser run observed one preserved overflow-style failure followed by 14 passes; the identical unmodified confirming run passed 15/15 and the full Playwright suite passed all 47 scenarios. No TaskTable, overflow hook, overflow test, or styling file was changed by the auth focus fix.
- `src/auth/AuthApp.test.tsx` remains an inherited large integration test module at 290 pure LOC. Splitting it was intentionally not mixed into this minimal focus remediation because the requested exact regression path and shared-worktree scope had to remain stable.

## 2026-09-05 Task Overflow Pre-growth End Completion

- The first contraction-aware candidate was incomplete: the formerly flaky scenario still failed once in 60 runs because `End` could be handled before rendered overflow existed. The deterministic component regression reproduced the same transition as `scrollLeft=0` instead of `420`.
- Preserving a single boolean intent through all no-overflow states then regressed the existing responsive contract `10/10`: after overflow was deliberately removed and restored at 1024px, Right remained disabled instead of exposing the left boundary. Modeling pending versus anchored intent resolved both contracts without waits, retries, timeout changes, or weakened assertions.
- No task-table overflow blocker remains after `112/112` Vitest tests passed twice, the exact reported Chromium command passed `30/30`, the final focused scenario passed `60/60`, and the complete Playwright suite passed `47/47`. Production builds retain only the unchanged non-fatal Rollup warnings for Zod dependency comments.
