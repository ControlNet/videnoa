# Rust 1.100 and the 2026-11-15 dependency advisories

## Why

CI's "Workspace quality gate" failed on `cargo deny check advisories` after
new RustSec advisories were published. All other CI jobs were green.

## Dependencies

| Crate | Before | After | Note |
|---|---|---|---|
| openssl / openssl-sys | 0.10.75 / 0.9.x | 0.10.82 / 0.9.118 | `cargo update -p openssl` |
| futures family | 0.3.31 | 0.3.34 | futures, futures-util, -core, -channel, ... |
| hyper | 1.8.1 | 1.12.0 | drops `pin-utils`; `want` 0.3.2 |
| rpassword (+ rtoolbox) | 7.3.1 | removed | unused since 4998af0 (`--password` prompt dropped) |
| renamore | any target | `cfg(windows)` only | only `publication_finalizer.rs`'s Windows path calls it |

`RUSTSEC-2026-0459` (bundled SQLite < 3.54 leaks page-cache memory when it
opens a database and `-wal` written by a less trusted party) is ignored in
`deny.toml`. Videnoa opens only its own `jobs.db` and the controller
database. The fix needs `libsqlite3-sys >= 0.39`, but sqlx 0.8.6 and 0.9.0
require `< 0.38`, and Cargo allows only one `libsqlite3-sys` in the graph
(rusqlite in core and sqlx in the controller must agree). Drop the ignore once
sqlx accepts 0.39.

## Toolchain 1.98.0 -> 1.100.0

The pinned version appears in all of these; change them together:

- `Cargo.toml` `workspace.package.rust-version`, `rust-toolchain.toml`
- `Dockerfile`, `Dockerfile.controller` (`rust:1.100.0-bookworm`, checked on
  Docker Hub before the bump)
- `.github/workflows/unittest.yaml`, `release.yaml`: the toolchain input and
  the pinned `dtolnay/rust-toolchain` SHA. The SHA is the commit of that
  action's `1.100.0` ref (`d5e4d57b030548f435d0befec15ff413a2c60eff`).
- `README.md`, `scripts/check_controller_container.sh`,
  `crates/controller/tests/package_contract.rs`

`node scripts/tests/validate_ci_release_workflows.test.mjs` checks the
workflows (rules in `scripts/workflow_contracts/`); run it after a bump.

Rust 1.83 is not a target any more. `cargo-rust-1.83-lockfile.md` is
superseded.

## New lints in 1.100

- `clippy::map_unwrap_or` now flags `.map(f).unwrap_or_default()`; use
  `map_or_else(String::new, f)`.
- `clippy::assert_is_empty` (pedantic, denied in controller and protocol)
  flags `assert!(x.is_empty())`. Use `assert_eq!(x, [])`. When the element type
  cannot be inferred from `[]`, name it: `Vec::<String>::new()`.
  `cargo clippy --fix` writes `[] as [T; 0]`; shorten that by hand.
- Cargo's new `cargo::unused_dependencies` lint (warn) reports `rust-embed` in
  core and controller. It is a false positive for debug builds: the embedded
  frontend is `#[cfg(not(debug_assertions))]`. It is a manifest warning, so
  `-D warnings` in clippy does not turn it into an error. Keep the dependency.

## Compatible updates (second pass, same day)

`cargo update` moved 248 lockfile entries within the declared ranges (tauri
2.10.2 -> 2.12.5, wry 0.54 -> 0.57, ...). The desktop build regenerates
`crates/desktop/gen/schemas/*.json` from tauri; commit them with the lockfile.
Tauri 2.12 adds a few read-only commands to its default permission sets
(`core:app` supports-multiple-windows, `core:window` activity-name and
scene-identifier, `core:tray` set-icon-with-as-template).

`npm update` in `web/` and `controller-web/` stays within package.json ranges
(React 19.3, Radix, tailwind 4.3, vite 7.3.8, Playwright 1.65, ...). Two
follow-ups:

- `eslint-plugin-react-hooks` 7.1 adds `set-state-in-effect` and `refs` errors.
  `web/` first kept 7.0.1, then fixed the five places and moved to 7.1.1:
  PresetGallery, RunWorkflowDialog and BatchPanel now adjust state during
  render when `open` or the workflow inputs change (React's "storing
  information from previous renders" pattern: keep the previous value in
  state and compare). ComparisonViewer read `isPanning.current` for the
  cursor, which stayed "grab" while dragging; a `panning` state now drives it.
- Radix `react-dismissable-layer` 1.2 (via react-dialog 1.3) dismisses on the
  click that follows a primary-button outside `pointerdown`, not on the
  `pointerdown`. A jsdom test that fires only `pointerDown` no longer closes
  the dialog; fire `pointerDown`, `pointerUp` and `click`.

Exact pins bumped (third pass, still exact): `cap-std`/`cap-fs-ext` 3.4.6,
`glob` 0.3.4, `rust-embed` 8.14.0, controller `reqwest` 0.12.28, `iroh` 1.4.1
(`iroh-proxy-utils` 0.3.0 accepts `iroh ^1`; 1.4 drops the hickory DNS
crates). Most of these pins date from the Rust 1.83 support period. A release
`cargo check` covers the `rust-embed` derive, which debug builds skip.

Still behind, each needs code changes: `ort` rc.15 (keeps `api-23`, so the
bundled ONNX Runtime 1.23.2 stays; needs GPU testing), `sqlx` 0.9, `rusqlite`
(at most 0.39 next to sqlx: libsqlite3-sys < 0.38), `rustix` 1.x, `toml` 1.x,
`cap-std` 4, `reqwest` 0.13, RustCrypto majors (`argon2` 0.6: check stored
hashes still verify); frontend majors vite 8, eslint 10, TypeScript 7,
react-router 8, lucide-react 1.x. RUSTSEC-2026-0459 stays ignored until sqlx
accepts libsqlite3-sys 0.39.

Dev-only npm audit: typescript-eslint 8.72.2 still bundles minimatch < 11.1
(GHSA-g3hv-75wg-m249, moderate). Runtime `npm audit --omit=dev` is clean.

## Verification

```bash
export ORT_DYLIB_PATH=$PWD/lib/libonnxruntime.so
export TRT_LIBS=$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs
export LD_LIBRARY_PATH=$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:$LD_LIBRARY_PATH
rustc --version   # 1.100.0
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo deny check advisories
cargo test --locked -p videnoa-core -p videnoa-app -p videnoa-protocol -p videnoa-transport
cargo test --locked -p videnoa-controller --all-features
```
