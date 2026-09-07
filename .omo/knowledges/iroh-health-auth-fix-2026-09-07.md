# Health authentication classification correction — 2026-09-07

Supersedes the classification gap recorded in `iroh-review-assessment-2026-09-07.md`.
Implementation started at af3af998ed9497b9cd60caa64711c5ce7677ec57.

## Implemented scope

`workers/health/probe.rs` now uses the same small `classify_error` helper for
health and capability client failures. ClientStatus 401/403 becomes Authentication;
all other failures preserve the stage-specific fallback. No worker authentication,
transport protocol, rate limiter, or health service scheduling changes were needed.
CONNECT authenticates before the public health HTTP request, so health client
errors must include authentication classification even when HTTP health is public.

The existing real CONNECT client unit fixture now also runs a health service
against an isolated SQLite iroh registration. Its test-only admission callback
counts rejected credentials. A wrong saved password produces the authentication
message; over 3.2 seconds with a one-second cadence/retry setting, the failure
counter and persisted record version remain unchanged. Editing the saved password
increments the version; a fresh cadence brings the worker online with no additional
authentication failures. Fixtures are synthetic and do not instantiate the worker's
Argon2 rate limiter; production worker authentication code is unchanged.

Before the code fix, the new regression failed with actual `worker health check
failed` versus expected `worker authentication failed; check the saved worker
password`. After the fix it passed. Unit coverage also verifies both stages for
401/403 and preserves Network, Timeout, Stall, MalformedPayload, RateLimited,
ClientStatus 429 and ServerStatus 503 fallbacks. Existing task20 HTTP health
regressions verify authentication blocking/password edits and transient retries.

## Verification commands

Run from the repository root; selected tests should all pass and Clippy should
produce no warnings:

```bash
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-controller --lib
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-controller --test task20 worker_health
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-controller --test iroh_registration --test worker_password
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## Windows proposal only — not applied

An isolated copy at `/tmp/videnoa-wmi-resolution-idalhiwg` tested changing only
wmi's Cargo.lock dependency edge from `windows 0.61.3` to the existing
`windows 0.62.2`. This matches its direct `windows-core 0.62.2` dependency and
satisfies wmi's declared >=0.59,<0.63 version range. No package versions were
added/removed/upgraded; other users of windows 0.61.3 retained that dependency.
`cargo tree --locked --offline --target x86_64-pc-windows-msvc -p wmi --depth 2`
accepted the resulting graph. Comparing Linux transport dependency trees before
and after yielded identical results, excluding the temporary root path.

This is dependency-resolution evidence only, not proof of Windows compilation
or runtime behavior. Actual Windows Rust CI is still required before accepting
the Windows fix. The real workspace Cargo.lock and version pins were not changed
in this follow-up. In particular iroh remains =1.1.0 and proxy-utils =0.3.0.
