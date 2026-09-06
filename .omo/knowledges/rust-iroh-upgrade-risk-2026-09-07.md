# Rust and dependency upgrade risk for iroh TCP forwarding

Assessment date: 2026-09-07. No toolchain settings or dependencies were changed.

## Corrected baseline

The workspace's Rust 1.83 declaration is inherited only by Controller. Core,
app, and desktop use edition 2021 without declaring an MSRV. The current locked
`ort` and `ort-sys` rc.12 and `libloading` 0.9 require Rust 1.88. Worker Docker
uses Rust 1.88; Controller Docker uses 1.83. Worker/desktop CI uses floating
stable, while Controller jobs pin 1.83. The local default is Rust 1.95 nightly.
There is no repository toolchain file. A change must distinguish build-toolchain
pinning from the minimum version advertised to downstream builders.

Cargo.lock is tracked. Earlier historical knowledge claiming it is ignored is
stale. Preserve it when testing compiler-only changes.

## Evidence collected

These read-only commands passed using the already installed Rust 1.91.0:

```bash
cargo +1.91.0 metadata --locked --offline --format-version 1 --no-deps
cargo +1.91.0 metadata --locked --offline --format-version 1
cargo +1.91.0 tree --locked --offline -i libsqlite3-sys
```

The returned package metadata had no declared MSRV above 1.91. This establishes
manifest/dependency resolution compatibility, not successful compilation or
runtime correctness; packages may omit or inaccurately declare their MSRV.
Both reqwest 0.12.27 and 0.13.2 already appear in the resolved metadata.
SQLx 0.8.6 and rusqlite 0.32.1 currently share libsqlite3-sys 0.30.1.

## Risk assessment (engineering judgment, not measured probabilities)

| Scope | Risk | Main validation burden |
| --- | --- | --- |
| Pin a supported newer Rust, keep lock/edition/runtime libraries | Low to medium | New compiler/Clippy diagnostics, cross-platform builds, release performance |
| Add iroh/proxy with narrowly required dependency changes | Medium | QUIC/TLS/crypto build graph, networking behavior, native packaging |
| Compatible updates of ordinary Rust libraries in small groups | Low to medium | Behavioral changes and feature unification |
| Major/minor-breaking updates across networking, SQLite, Tauri | Medium to high | API migrations, persistence compatibility, native desktop dependencies |
| Update ort plus CUDA/cuDNN/TensorRT/ONNX Runtime together | High | API/ABI/provider alignment, inference output/performance and engine caches |

Iroh-proxy-utils 0.3.0 depends on iroh 1; selecting iroh 1.1.0 requires Rust
1.91. This does not imply upgrading all application dependencies. Cargo permits
semver-incompatible crate versions to coexist when other constraints permit;
keep their Rust types separated. Native `links` constraints are different:
upgrading rusqlite independently from SQLx can make their sqlite3 bindings
unresolvable. Recheck the shared native graph before either upgrade.

Rust edition migration is independent: keep Videnoa on edition 2021 initially,
even when dependencies use edition 2024. Keep `resolver = "2"` initially too.
The project has already encountered cross-version Clippy group differences and
denies all/pedantic warnings in Controller. Expect lint fixes to be a practical
part of toolchain unification.

Keep ort pinned to rc.12 with api-23 and ONNX Runtime 1.23.2 for the networking
work. A Rust upgrade alone does not require upgrading CUDA, TensorRT, FFmpeg,
models, npm packages, or SQLite schema. Compiler optimization changes still
justify a representative inference output/performance check.

## Recommended sequence

1. Pin an explicit supported stable build toolchain at least 1.91; decide the
   advertised MSRV separately. Align CI, Docker, and developer tooling. Preserve
   lockfile, edition, GPU runtimes, and OS base-image family for this change.
2. Run locked build/test/lint gates and Linux/Windows packaging. Verify existing
   database reopening, HTTP task lifecycle, transfer/cancel/recovery, desktop
   startup, and representative GPU output/performance. Do not claim metadata
   success is a substitute for these gates.
3. Add TCP forwarding dependencies; update only graph entries required by the
   chosen versions. Validate real NAT and relay behavior separately.
4. Treat optional dependency modernization as subsequent changes grouped by
   networking, database, desktop, or GPU concerns, each independently revertible.

## Sources

- [Rust edition interoperability](https://doc.rust-lang.org/edition-guide/editions/index.html)
- [Cargo dependency and native links resolution](https://doc.rust-lang.org/cargo/reference/resolver.html)
- [Targeted Cargo updates](https://doc.rust-lang.org/cargo/commands/cargo-update.html)
- [Rust release and compatibility notes](https://doc.rust-lang.org/stable/releases.html)
- [Proxy dependency manifest](https://docs.rs/crate/iroh-proxy-utils/0.3.0/source/Cargo.toml)
- Repository manifests, Dockerfiles, unittest/release workflows, and knowledge
  files `cargo-rust-1.83-lockfile.md`,
  `controller-clippy-cross-version-compatibility-2026-09-04.md`, and
  `ort-rc12-api23-migration-2026-08-24.md` (historical lockfile statement corrected).
