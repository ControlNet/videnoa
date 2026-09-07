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
