# Controller Clippy Cross-Version Compatibility

## Problem

The Controller supports Rust 1.83 while development also runs a newer toolchain. Clippy lint groups are not stable sets across releases: Rust 1.83 enables `module_name_repetitions` through `pedantic`, while the current toolchain classifies it differently. Consequently, identical package lint configuration can produce different diagnostics.

## Stable Pattern

Do not suppress `module_name_repetitions`. Keep strict groups and `-D warnings` unchanged. Instead, name private implementation modules by responsibility and preserve established public paths with façade modules that re-export the implementation surface.

Rust's `#[path = "..."]` attribute separates a module's logical name from its source filename. For example, a source file named `remote/client.rs` can be loaded as private module `connector`, while public module `remote` continues to export `VidenoaClient`. This changes neither the Rust item name nor any serde, SQLx, Clap, HTTP, or database value.

At the crate root, an included topology file can hold private implementation declarations and public façade modules while preserving root-level module semantics. This also prevents module declarations from pushing an otherwise focused `lib.rs` over the 250 pure-LOC ceiling.

Genuine diagnostics should be corrected. For value-returning branded-type methods, `#[must_use]` communicates the API contract and satisfies `must_use_candidate` on both toolchains. Continue rerunning the old-toolchain command after each batch because removing dominant diagnostics can reveal masked findings.

## Verification

Use the exact package gates on both toolchains:

```bash
cargo +1.83.0 clippy -p videnoa-controller --all-targets --all-features -- -D warnings
cargo clippy -p videnoa-controller --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo test -p videnoa-controller --all-targets
git diff --check
```

For tests using `PathCapabilities::open`, create every configured root before opening capabilities. The API retains descriptors for `input_roots`, `output_roots`, `data_root`, and `temp_root`, so fixtures that omit any configured directory do not satisfy the production contract.

Shared integration-test support must live below the owning test module rather than directly under `tests/`, because every top-level `.rs` file in that directory is compiled as a standalone integration target. Split oversized integration tests by behavior and keep shared fixtures in a nested support module.

## Final LOC Extraction Map

- `tests/mock_videnoa/recovery_support.rs` owns recovery fixture/bootstrap and task loading; `tests/mock_videnoa/recovery_support/state_builder.rs` owns remote lifecycle state synthesis.
- `tests/task12/support.rs` owns fixture/bootstrap, executor/client construction, and persistence lookup; `tests/task12/support/task_lifecycle.rs` owns task progression; `tests/task12/support/artifact_paths.rs` owns verified/partial/evidence paths and zero jitter.
- `src/main.rs` owns CLI and runtime composition; `src/termination.rs` owns runtime exit classification and shutdown-signal selection.

Audit modified and untracked Controller Rust paths with NUL delimiters from Git through sorting and reporting. Partial file lists are insufficient: the final complete audit found an additional 251-line entrypoint after the two reported test-helper defects. The final maximum is 244 pure LOC.
