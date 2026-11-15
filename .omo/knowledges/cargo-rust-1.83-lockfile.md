# Cargo Rust 1.83 Compatibility

> **Superseded.** Rust 1.83 is no longer supported. Since 2026-09-07 every
> crate inherits the workspace `rust-version` (1.98.0, then 1.100.0 from
> 2026-11-15; see `rust-1.100-and-dependency-advisories-2026-11-15.md`). The
> lockfile also no longer parses with Cargo 1.83: `cpufeatures` 0.3.1 uses
> edition 2024. Do not use 1.83 as a check.

- `workspace.package.rust-version = "1.83"` is a selective default. Only `videnoa-controller` inherits it and advertises Rust 1.83 support.
- Core, app, and desktop intentionally do not inherit that value. Exact pre-existing `ort-sys 2.0.0-rc.12` requires edition-2024 Cargo support, so Cargo 1.83 cannot parse the GPU core graph.
- This repository builds executable products, so `Cargo.lock` must be tracked. Without it, Cargo 1.83 resolves and parses newer inactive optional packages before compiling the selected package.
- SQLx 0.8.6 with `default-features = false` and `sqlite`, `migrate`, and `macros` activates only SQLite in the Controller feature graph. MySQL/Postgres packages may still appear as inactive lock metadata.
- `rust-embed` 8.12 switches its helper crates to an edition-2024 dependency chain that Cargo 1.83 cannot parse. Keep `rust-embed = "=8.11.0"` and its lockfile entries at 8.11.0 while Rust 1.83 remains supported.
- The compatible lock also keeps `base64ct` at 1.7.3, `home` at 0.5.11, and `time` at 0.3.45. This lock guarantee is Controller-scoped; verify with `rustup run 1.83.0 cargo test -p videnoa-controller --all-targets --locked`.
