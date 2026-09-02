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

## 2026-09-03 Task 14 Oracle Blockers

- Processing retry accepted terminal jobs with contradictory or absent identity fields, then deleted the workspace and created a replacement attempt. It now reuses the recovery identity predicate before any cleanup.
- Status counts exposed sparse SQL groups directly. The API now returns all fourteen statuses in deterministic lifecycle order, including zero counts.
- Cancel/retry handlers duplicated lifecycle SSE publication and reloaded the task after commit, allowing publication/read failure to turn durable success into HTTP 500. The redundant handler path was removed.
