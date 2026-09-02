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
