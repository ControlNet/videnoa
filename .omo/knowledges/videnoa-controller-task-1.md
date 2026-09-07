# Videnoa Controller Task 1 Knowledge

## Build Boundary

- `crates/controller/build.rs` must do frontend work only in the release profile.
- The release sequence is locked to `npm ci --no-fund` followed by `npm run build` in `controller-web/`.
- Debug compilation must not require npm or generated assets; runtime startup validates `controller-web/dist/index.html` instead.
- Release Rust code embeds generated assets with `rust-embed`, while debug Rust code owns a validated disk directory served by `tower-http`.

## Isolation Boundary

- Controller dependencies are limited to HTTP/runtime/serialization/static-serving concerns.
- Keep a test that executes `cargo tree -p videnoa-controller --edges normal,build` and rejects `videnoa-core`, `ort`, CUDA, cuDNN, TensorRT, and model-runtime names.
- Root workspace dependency declarations do not create a dependency by themselves; only entries selected in `crates/controller/Cargo.toml` enter the Controller graph.

## QA Boundary

- Debug startup failures are typed and tested for both missing directory and missing index cases.
- Release tests must request a nested route from the embedded router so SPA fallback and embedding are proven together.
- Live QA should request `/api/health`, `/`, and a nested route, then capture 1280, 768, and 375 pixel browser evidence with console/network inspection.

## Independent QA Result (2026-09-02)

- Commit `6f23c16f6ed4260f2c428616542dee31761c1d17` passed Controller formatting, clippy, tests, fresh GPU-free debug/release builds, npm gates, dependency/dynamic-link isolation, typed asset failures, embedded release serving, live HTTP, responsive browser rendering, and cleanup.
- The overall Task 1 verdict is `FAIL` because dark `--color-accent: oklch(0.58 0.22 292)` is `4.12:1` against the dark surface for 12px/14px text, below the WCAG 2.2 AA `4.5:1` requirement.
- Browser measurement found `oklch(0.61 0.22 292)` reaches `4.69:1`; after changing the product token, regenerate both dark captures and rerun independent visual review.
- Complete approving and failing evidence is recorded under `.omo/evidence/videnoa-controller/task-1/review/qa/`.

## Fix Round 1 Result (2026-09-02)

- Keep `videnoa-controller` on edition 2021 so Cargo 1.83 can parse workspace metadata; verify package discovery directly with `rustup run 1.83.0 cargo metadata --no-deps --format-version 1`.
- Register `/api/{*path}` with a minimal JSON 404 handler before either debug or release SPA fallback. This protects the API namespace without pulling Task 2's error architecture forward.
- Dark `--color-accent: oklch(0.61 0.22 292)` resolves to RGB `(139, 94, 249)` in Chromium and measures `4.6943:1` against the dark surface RGB `(6, 13, 25)`.
- Preserve visual evidence by comparing all 11 current screenshots against the approved baseline: light captures should remain pixel-identical, while dark diffs should be limited to the intentional accent pixels.

## Encoded Static Asset Boundary (2026-09-02)

- Keep raw URI parsing, percent syntax validation, UTF-8 decoding, unsafe-character rejection, and API classification in one outer middleware pass.
- Preserve the decoded value as a typed `DecodedPath` request extension. Debug `ServeDir` continues handling the original URI, while release embedded lookup consumes the extension so canonical and safely encoded asset spellings resolve identically.
- Do not recursively decode. A double-encoded `/api%252Funknown` remains a SPA route after one pass, while `/api%2Funknown` returns the exact JSON API 404.
- Cross-profile live verification must compare canonical and encoded asset bytes/content type, exercise malformed/backslash/NUL/control paths, verify GET/HEAD SPA behavior and POST/OPTIONS 405 responses, and inspect browser console/network loading.

## Canonical Decoded Path Policy (2026-09-02)

- Parse the percent-decoded path once into typed success/error outcomes before API classification. Reject malformed encoding, invalid UTF-8, backslashes, controls, exact `.`/`..` segments, and internal empty segments with empty 400.
- Permit `/`, one trailing slash on an ordinary route, Unicode, query punctuation, and safe encoded separators that produce non-empty ordinary decoded segments.
- Never resolve parent segments, recursively decode, or delegate canonicalization to `ServeDir`/`rust-embed`; both profiles must receive only a boundary-approved `DecodedPath`.
- Static asset verification must dynamically discover release filenames and prove `text/javascript`, non-empty body, bytes distinct from SPA index, and canonical/encoded-hyphen/encoded-slash equality.

## Windows Device Alias Policy (2026-09-02)

- Exact asset components must reject case-insensitive `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM1`-`COM9`, `LPT1`-`LPT9`, `COM¹`-`COM³`, and `LPT¹`-`LPT³`, including extensions and normalized trailing-space/dot forms.
- Parse numbered device suffixes as Unicode scalar strings rather than assuming a four-byte UTF-8 basename.
- Keep pure eligibility tests profile- and host-independent. Only Unix debug tests may create real files with foreign drive, ADS, or reserved-device spellings.
