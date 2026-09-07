# Windows WMI resolution correction — 2026-09-07

Supersedes the proposal-only Windows section of `iroh-health-auth-fix-2026-09-07.md`.

## Root cause and bounded fix

Starting revision: af3af998ed9497b9cd60caa64711c5ce7677ec57.
The failing Windows graph was iroh 1.1.0 -> netwatch 0.19.3 -> wmi 0.18.4
(also iroh -> portmapper 0.19.3 -> netwatch). Wmi resolved windows 0.61.3,
which used windows-core 0.61.2/windows-result 0.3.4, while its separate direct
windows-core dependency resolved 0.62.2/windows-result 0.4.1. Its independently
specified >=0.59,<0.63 ranges allowed this inconsistent combination. Netwatch
itself already used windows 0.62.2. Multiple crate families elsewhere in the
workspace are valid; crossing the two type families inside wmi caused the errors.

Cargo.lock changes exactly one dependency edge: wmi -> windows 0.61.3 becomes
wmi -> windows 0.62.2. All package versions/checksums and manifests remain
unchanged. Other consumers of windows 0.61.3 retain it. No iroh pins, features,
protocols or CI jobs were changed. Cargo validates this resolution with --locked.
The Linux transport dependency graph remains identical, because the affected edge
is Windows-only. Future dependency updates should retain coherent wmi resolutions;
Windows CI must remain enabled to catch regressions.

## Evidence and commands

The original failing Windows Rust job is:
https://github.com/ControlNet/videnoa/actions/runs/34088881514/job/101638178288
Its logs show IWbemObjectSink/windows_core::Interface and HRESULT/error type
mismatches during wmi compilation, before Videnoa tests. The same run's Windows
archive job also failed and is inspected separately.

```bash
cargo tree --locked --target x86_64-pc-windows-msvc -p wmi --depth 2
CARGO_TARGET_DIR=/tmp/videnoa-iroh-win-target cargo check --locked --target x86_64-pc-windows-msvc -p wmi
CARGO_TARGET_DIR=/tmp/videnoa-iroh-win-target cargo check --locked --target x86_64-pc-windows-msvc -p videnoa-transport
CARGO_TARGET_DIR=/tmp/videnoa-iroh-win-target cargo check --locked --target x86_64-pc-windows-msvc -p videnoa-core -p videnoa-app
git diff --check
```

Wmi cross-check passed in 16.50 seconds with the corrected lockfile. Complete
transport/core/app cross-checks cannot complete on this Linux host: native Windows
C build tools (including MSVC lib.exe) are unavailable, and AWS-LC's Windows C
sources cannot be compiled by the host GNU compiler. These are tooling limitations,
not Windows compilation or runtime acceptance. The actual GitHub Actions Windows
Rust job remains the authoritative verification; its post-push result will be
recorded separately rather than predicted here.

## Post-push verification

Code commits:
- 3b29087: classify tunnel authentication during health probes.
- 96e068448b53c14ea7fe8ea57801f9148fc99b9d: Windows dependency resolution.

Run: https://github.com/ControlNet/videnoa/actions/runs/34091257601
Windows Rust job **passed**:
https://github.com/ControlNet/videnoa/actions/runs/34091257601/job/101645016795
The completed job log confirms successful execution of:
- authentication_rotation_and_shutdown_preserve_tcp_semantics;
- sse_is_delivered_incrementally_and_arbitrary_targets_are_rejected;
- reset_disables_persisted_iroh_without_replacing_identity;
- iroh_password_and_api_lifecycle.

This proves Windows compilation and automated loopback iroh execution, including
password lifecycle and streaming. It does not prove Windows deployment over real
NAT/CGNAT or public-relay paths. Different-network NAT, CGNAT, forced-relay behavior,
representative large-video throughput/control latency, and production Windows
network conditions remain release acceptance gaps.

Linux validation passed:
- transport: 4 passed, 1 ignored (public N0 test not repeated);
- Controller: 563 passed, 1 ignored;
- core --lib --tests: 628 passed, 10 ignored;
- all-workspace/all-targets/all-features Clippy with -D warnings;
- web: 192 tests, lint and build;
- controller-web: 146 tests, lint and build;
- cargo fmt --all -- --check and git diff --check.

Commands used:
```bash
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-transport
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target RUST_TEST_THREADS=2 cargo test --locked -p videnoa-controller
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target RUST_TEST_THREADS=2 cargo test --locked -p videnoa-core --lib --tests
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
npm --prefix web test
npm --prefix web run lint
npm --prefix web run build
npm --prefix controller-web test
npm --prefix controller-web run lint
npm --prefix controller-web run build
cargo fmt --all -- --check
git diff --check
```

CI status when recording this evidence: Windows/Ubuntu Rust matrix jobs, both
Web build jobs, Controller web quality/E2E and workflow contracts passed.
Controller Rust quality/tests and package/Docker jobs were still running; the
whole workflow was not yet green. No new failed job had been reported. The old
Windows archive failure was separately inspected and had the same wmi type
mismatch; its replacement archive job must be judged by its own final result.
