# F2 Code Quality, Security, and Data-Integrity Review

Audit date: 2026-09-05

Audited tip: `d8830fa55cf54d504eafbc768747e57f3d2dcadf`

Shell-remediation commit: `90561edafb7df40af084c3ffe8a19bebf909c3c9`

Hosted authority: [GitHub Actions run 33919997302](https://github.com/ControlNet/videnoa/actions/runs/33919997302)

## Verdict

**REJECT**

The exact tip passes the Controller Rust, frontend, dependency, security, load, fault, archive, and image gates exercised by this review. The shell remediation is behaviorally verified and the hosted run's only failure is an unrelated legacy Linux package split-archive step. However, F2 requires rejection of panic-based request handling and requires startup reconciliation to cover every durable nonterminal task. The current tip violates both requirements: authenticated middleware can panic when peer connection metadata is absent, and recovery/orchestration scan only the first 65,535 nonterminal rows without pagination.

## Blocking Findings

### 1. Authenticated request handling contains a reachable panic boundary

**Severity:** HIGH

- `crates/controller/src/auth/boundary.rs:17-24` implements `peer_ip` by reading `ConnectInfo<SocketAddr>` from request extensions and calling `.expect("server requests include peer connection information")`.
- `peer_ip` is called by authenticated task and operations request middleware. A request routed through `controller_app_router` without the matching extension panics instead of producing a typed HTTP error.
- The production listener in `crates/controller/src/auth/http.rs:105-124` installs `into_make_service_with_connect_info::<SocketAddr>()`, so the normal binary wiring supplies the extension. That wiring does not remove the panic from the request path: `controller_app_router` is public and has multiple direct test/runtime consumers, while the F2 plan explicitly says to reject panic request paths.
- Required remediation: make peer metadata extraction fallible and map absence to a typed internal or authentication response, or encapsulate the router so every construction path statically guarantees the extension. Add a direct-router regression proving absence cannot panic.

### 2. Recovery can omit durable nonterminal tasks above 65,535 rows

**Severity:** HIGH

- `Store::recovery_tasks` in `crates/controller/src/persistence/task.rs:34-44` accepts a `u16` limit and executes one ordered `LIMIT ?` query with no cursor or offset.
- Startup reconciliation uses `RECOVERY_SCAN_LIMIT: u16 = u16::MAX` in `crates/controller/src/recovery/reconciler.rs:15,55-69`. It therefore processes at most 65,535 nonterminal tasks even though its contract says it reconciles every durable nonterminal task.
- Recurring orchestration uses the same `SCAN_LIMIT: u16 = u16::MAX` in `crates/controller/src/orchestration.rs:16,124-149`. Each fill reads the same oldest prefix. Active, deferred, or otherwise long-lived tasks are filtered only after the limited query, so they can continuously occupy that prefix and starve later tasks.
- This violates Task 10's requirement to scan every nonterminal task/attempt on startup. It can leave later durable tasks without recovery, transfer, publication, or cleanup after restart.
- Required remediation: paginate over a stable `(updated_at_ms, id)` cursor, or otherwise iterate until the complete nonterminal set has been considered. Filtering active/deferred tasks must not consume the page budget. Add a regression exceeding the page size and proving a task beyond the first page is reconciled and dispatched.

## Hosted Run Disposition

- Run `33919997302` is for the exact audited SHA and completed with overall `failure`.
- All F2-relevant jobs passed: Controller Rust quality/tests, Controller frontend quality/E2E, Controller fault/load/security suites, Linux and Windows Controller archives, Controller image/content smoke, workflow contracts, existing Rust tests, and web builds.
- The sole failed job was legacy `Package smoke (Linux)`. Its `Create split archive (2000MB volumes)` step failed after the bundle build and runtime checks; the hosted log reports p7zip 16.02 `System ERROR: E_FAIL`.
- This legacy packaging failure is not evidence of a Controller F2 defect, but the exact-tip hosted run must not be described as green.

## Passing Security and Integrity Areas

| Area | Result | Exact-tip basis |
|---|---|---|
| Authentication behavior | PASS with panic blocker above | Direct-peer login/Bearer throttling, session fallback, exact Bearer parsing, typed 401/403/429 responses, CSRF, and hostile-origin behavior pass focused and live checks. |
| Sessions and secrets | PASS | Argon2id verification, random session/CSRF material, digest-only persistence, expiry/rotation/revocation, protected cookies, and password-hash invalidation remain enforced. Secret-scan matches are synthetic fixtures, not usable credentials. |
| SQL, migrations, indexes, and CAS | PASS | Migration, rollback, constraints, optimistic versions, submission ownership, atomic reservation, snapshot pagination, and 20,000-row query-plan/load tests pass. |
| Lifecycle and remote submission | PASS with recovery blocker above | Exhaustive lifecycle, persist-before-side-effect, cancellation, durable submission ownership, restart takeover, and same-generation no-duplicate-request tests pass. |
| Input and temporary paths | PASS | Capability-retained roots, descriptor-relative/no-follow access, traversal/symlink/FIFO rejection, replacement detection, bounded streaming, and length/hash checks pass adversarial coverage. |
| Publication and no-clobber | PASS on Linux | No-replace finalization, descriptor-relative staging, racing destination preservation, duplicate finalizers, cross-filesystem publication, and staging-replacement rejection pass. Native Windows race behavior remains host-specific. |
| Unsafe and typed errors | PARTIAL | Controller forbids unsafe code and domain failures are typed, but missing peer connection metadata still uses panic-based request handling. |
| Frontend and shell | PASS | Lint, typecheck, 112 Vitest tests, production build, focused shell tests, and all 48 Chromium E2E scenarios pass. Wheel scrolling, control/alert non-overlap, and focus retention are asserted behaviorally. |
| Controller dependencies | PASS | Locked Controller paths use `anyhow 1.0.103`, `h2 0.4.16`, and `rustls-webpki 0.103.13`; no Controller normal/build path to GPU/core or the remaining desktop/core advisories was found. |
| Packaging boundary | PASS for Controller | Controller archive/image checks pass and remain GPU-free, independent of ORT, CUDA, cuDNN, TensorRT, model content, and `videnoa-core`. |

## Verification Performed

- Provenance: `HEAD == origin/dev == d8830fa55cf54d504eafbc768747e57f3d2dcadf`, divergence `0 0`, and clean worktree at audit start.
- `cargo +1.83.0 fmt --all -- --check`: PASS.
- Rust 1.83 Controller all-target/all-feature check, strict Clippy, and tests with the locked graph: PASS.
- Focused migration, query-plan/load, CAS, hostile path/auth, publication, and submission-ownership tests: PASS.
- Controller web install, lint, typecheck, 112 Vitest tests, production build, six focused shell tests, and all 48 Chromium E2E tests: PASS.
- LSP diagnostics for `controller-web/src/shell/AppShell.tsx` and `controller-web/tests/e2e/shell.spec.ts`: PASS.
- `npm audit`: zero vulnerabilities.
- `cargo deny check sources`: PASS.
- Advisory analysis: remaining reported vulnerability and unmaintained advisories terminate in desktop/core GTK3, Tauri, XML, time, or legacy Unicode paths; they do not reach the Controller normal/build graph.
- Controller archive/content/documentation contract scripts: PASS.
- Exact-tip hosted job and step status was rechecked with GitHub CLI.

## Tooling Limitations and Residual Risk

- Cargo 1.83 cannot parse workspace-wide `cargo tree --workspace --all-features` because `ort-sys 2.0.0-rc.12` uses Edition 2024. Feature-unified inverse dependency checks were rerun with current stable Cargo; Controller compilation and tests themselves pass on Rust/Cargo 1.83.
- Secret scanning found no real credential. The repository still lacks 19 common sensitive-file ignore patterns, including generic key and credential filenames; `.gitignore` hardening is recommended.
- Browser execution is Chromium-only. The shell uses modern `:has()` and dynamic viewport units, leaving cross-browser behavior as a residual compatibility risk.
- Native Windows publication races were not executed on this Linux host; implementation and hosted Windows package/archive coverage do not fully replace native filesystem race testing.
- The overall hosted run remains red until the independently scoped legacy Linux split-archive failure is resolved or superseded by a passing exact-tip run.

## Approval Conditions

1. Remove the panic-based peer metadata extraction from authenticated request handling and add direct-router regression coverage.
2. Replace the fixed-prefix recovery scans with complete pagination and prove recovery/dispatch beyond one page.
3. Rerun the focused regressions plus the complete Rust 1.83 Controller and frontend gates, then regenerate this exact-tip report.

VERDICT: REJECT
