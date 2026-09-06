# Repository Backlog Audit (2026-08-23)

## Highest Priority

1. Implement actual single-frame workflow execution in `POST /api/preview/process`; it currently returns the original frame URL and ignores the submitted workflow.
2. Upgrade vulnerable frontend dependencies. `npm audit` reports direct vulnerable versions of `react-router`, `vite`, and `uuid`; the full dependency tree also contains a critical `vitest` advisory.
3. Merge `dev` into `master` through review. `dev` contains the release verifier repository-context fix and beads cleanup; both branches currently have no protection rules.

## Quality Gates

- Add `npm test` and `npm run lint` to CI. Both pass locally (184 tests across 24 files; lint clean), but CI currently runs only `npm run build`.
- Fix existing `cargo fmt --all -- --check` drift in `crates/app/src/lib.rs`, then enforce formatting in CI.
- Establish and enforce a Clippy policy. `cargo clippy -p videnoa-core -p videnoa-app --all-targets -- -D warnings` currently reports 53 errors.
- Add desktop compilation coverage to ordinary CI.
- Add dependency scanning. GitHub Dependabot alerts are disabled and no code-scanning analysis exists.

## Product and Operations

- Add lifecycle cleanup/TTL for preview extraction directories and sessions.
- Validate preview frame filenames as generated `frame_NNNN.png` names even though the current Axum route captures a single path segment.
- Align CUDA fallback documentation/logging with actual provider failure behavior.
- Replace the stock `web/README.md` and document test, packaging, release, versioning, and Linux/Windows support contracts.
- Decide whether the frontend package version should follow the Rust workspace version.

## Verified State

- Latest `dev` GitHub Actions run completed successfully on Rust tests, web builds, Linux/Windows package smoke, and Docker smoke.
- No open GitHub issues or pull requests.
- `master` and `dev` have no branch protection.
- Local web tests and lint pass.
- Rust format and strict Clippy gates fail as described above.
