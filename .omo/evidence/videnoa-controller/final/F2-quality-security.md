# F2 Code Quality, Security, and Data-Integrity Review

Audit date: 2026-09-05

Audited tip: `094d87833ebbc55c8f27857cd70d85fc5cc91afe`

Repository baseline: `HEAD == origin/dev` at the audited tip. Verification ran in the clean detached worktree `/tmp/opencode/videnoa-f2-094d878`; concurrent edits to other Final Wave reports in the main worktree were excluded.

## Verdict

**REJECT**

The prior browser-determinism blocker is resolved: the exact Controller web suite passes all 47 Chromium scenarios, including the corrected overflow tests. Rust 1.83 formatting, strict Clippy, Controller tests, fault/load/security suites, documentation, archives, and hosted CI also pass. Two security blockers remain: Bearer authentication bypasses the failed-login limiter, and the production HTTPS worker-client dependency graph contains a known patched certificate-validation vulnerability.

## Blocking Finding

### F2-B1: Bearer authentication bypasses failed-login throttling

- `AuthService::login` records invalid password attempts in `LoginLimiter`, keyed by the connecting IP address, and returns `RateLimited` after the configured bound.
- `AuthService::authenticate_bearer` independently loads the password hash and performs Argon2 verification but never records failure, checks the limiter, or clears it after success.
- The authentication boundary calls this unbounded Bearer verifier for every protected route that receives `Authorization: Bearer ...`. An unauthenticated network client can therefore make unlimited online guesses and force repeated expensive Argon2 work through readiness, task, worker, settings, event, or logout endpoints.
- Existing rate-limit coverage proves only repeated `POST /api/auth/login` attempts. No integration test exercises invalid Bearer requests or expects a `429` response.

Required correction: pass the actual peer IP into Bearer authentication, apply the same bounded failure policy consistently across protected routes, clear the limiter after successful authentication, and add an integration test proving the sixth invalid Bearer request is rate-limited while valid Bearer and session authentication continue to work.

### F2-B2: Vulnerable TLS certificate validation dependency

- `Cargo.lock` pins `rustls-webpki 0.103.9` through `videnoa-controller -> reqwest 0.12.27 -> hyper-rustls 0.27.7 -> rustls 0.23.36`.
- `cargo deny check` reports four advisories for this version. RUSTSEC-2026-0099 is directly relevant to the Controller's accepted `https` worker URL path: wildcard certificates could be accepted despite a permitted DNS name constraint. The advisory is fixed in `rustls-webpki >=0.103.12`.
- Worker URLs explicitly permit HTTPS, and `VidenoaClient` builds the default rustls-backed `reqwest::Client`; the Controller has no certificate pinning or other mitigation that removes this validation path.
- The related URI-name-constraint advisory also requires a constrained or misissued certificate. The CRL matching and CRL parser advisories are not currently reachable because the Controller does not configure CRL validation. Those narrower conditions reduce practical likelihood but do not remove the known vulnerable DNS validation code from a production trust boundary.

Required correction: update the locked TLS dependency chain to a non-vulnerable `rustls-webpki` version, rerun Rust 1.83 Controller gates and `cargo deny`, and verify HTTPS worker communication against the corrected lockfile.

## Security and Integrity Dispositions

| Area | Result | Basis |
|---|---|---|
| Authentication and authorization | FAIL | Operational routes and cookie CSRF/origin checks are enforced, but Bearer password verification bypasses the only failed-login limiter as described in F2-B1. |
| Sessions and secrets | PASS | Argon2id verification and digest-only token/CSRF persistence are enforced. The tracked secret scan's two hits are synthetic negative/redaction fixtures, not usable credentials. |
| CSRF and CORS | PASS | The 46-test security target proves cross-origin preflight receives neither authorization origin nor credential headers; mutation tests reject missing same-origin or CSRF proof. |
| SQL, migrations, indexes, and CAS | PASS | Fresh/idempotent/rollback migrations, enum and relation constraints, optimistic versions, submission ownership CAS, atomic reservations, snapshot pagination, and the 20,000-row query-plan tests pass. |
| Lifecycle and recovery | PASS | Exhaustive transition and recovery tables, bounded retries, cancellation boundaries, restart matrices, malformed-task isolation, durable submission ownership, and same-generation no-duplicate-request tests pass. |
| Input and temporary paths | PASS | Retained root capabilities, no-follow input reopening, traversal/symlink/FIFO rejection, replacement attacks, and owned cleanup tests pass. |
| Publication and no-clobber | PASS on Linux | Descriptor-relative staging and finalization, regular-file checks, racing-destination preservation, duplicate finalizers, and cross-filesystem publication tests pass. Hosted Windows archive smoke passes; native Windows publication races remain a host-specific residual risk. |
| Panic, unsafe, and typed errors | PASS | `unsafe_code` is forbidden. Production search found no `unwrap`, `expect`, `todo`, or `unimplemented`; the sole `panic!` is in an internal test module. |
| Module size | PASS | The largest production Rust/TypeScript module is 250 nonblank/non-line-comment lines; no production file exceeds the established ceiling. |
| Frontend dependencies | PASS | `controller-web` install and `npm audit --json` report zero vulnerabilities. |
| Rust dependencies | FAIL | `cargo deny check` identifies the production-reachable `rustls-webpki` vulnerability described in F2-B2. `h2`, `quick-xml`, `time`, GTK, and Tauri findings belong to other workspace products and are not in the Controller normal/build graph. |
| Documentation and packaging | PASS | Controller docs, archive root/content contracts, deterministic Linux archive contracts, workflow contracts, and exact-tip hosted archive/container jobs pass. |

## Verification Performed

- Provenance: main `HEAD == origin/dev == 094d87833ebbc55c8f27857cd70d85fc5cc91afe`; detached audit worktree remained clean.
- Hosted run `33873102244`: all 14 jobs succeeded at the exact audited SHA, including Controller Rust, fault/load, web E2E, Linux/Windows archives, container smoke, and workflow contracts.
- `cargo +1.83.0 fmt --all -- --check`: PASS.
- `cargo +1.83.0 clippy -p videnoa-controller --all-targets --all-features -- -D warnings`: PASS.
- `cargo +1.83.0 test -p videnoa-controller --all-targets`: all completed targets passed, including Task 20 at 30/30; the aggregate shell exceeded its outer timeout only after later targets had completed.
- Split Task 21 load/concurrency/filesystem/resource/security command: 94/94 PASS (`7 + 18 + 22 + 1 + 46`).
- `cargo +1.83.0 doc -p videnoa-controller --no-deps`: PASS.
- Controller web `npm ci`, lint, build, and unit tests: PASS; 108/108 unit tests.
- Isolated `npm run test:e2e`: PASS; 47/47 Chromium tests. A prior concurrent run lost its preview server and produced connection-refused infrastructure failures; the isolated rerun supersedes it.
- `node scripts/tests/validate_ci_release_workflows.test.mjs`: PASS for all positive and negative workflow mutations.
- `bash scripts/tests/controller_docs_test.sh`: PASS.
- `bash scripts/tests/controller_archive_root_files_test.sh` and `bash scripts/tests/package_controller_test.sh`: PASS.
- Rust and TypeScript LSP diagnostics: zero diagnostics in scanned Controller source trees.
- Tracked secret scan: two reviewed synthetic fixture matches; no private key, token, credential, or password value found. `.env` is ignored; generic key/credential filename patterns are not comprehensively ignored.
- Independent security review confirmed the Bearer verifier does not use `LoginLimiter`; existing auth tests cover only login-endpoint throttling.
- `cargo deny check`: FAIL for advisories and an unconfigured license allow-list; advisory F2-B2 is blocking. The license result is not independently dispositive because the repository has no `deny.toml` policy.

## Residual Risk

- Generic secret-bearing filename patterns such as `*.pem`, `*.key`, and `credentials.json` are not covered by `.gitignore`; repository content is currently clean, but ignore hardening is recommended.
- Native Windows no-clobber behavior is covered by implementation review and hosted archive smoke, not by the Linux-local filesystem race tests.
- The full-workspace lockfile contains additional advisories in desktop/legacy product dependency trees. They are outside the Controller normal/build dependency graph but should be handled by their owning release reviews.

VERDICT: REJECT
