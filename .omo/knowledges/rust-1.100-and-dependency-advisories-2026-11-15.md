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
