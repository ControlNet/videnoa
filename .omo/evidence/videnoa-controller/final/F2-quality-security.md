# F2 Code Quality, Security, and Data-Integrity Review

Audit date: 2026-09-05

Audited tip: `67620f5b45d54205b44861e3cc5d59e88724e998`

Comparison baseline only: `094d87833ebbc55c8f27857cd70d85fc5cc91afe`

Hosted authority: [GitHub Actions run 33910972161](https://github.com/ControlNet/videnoa/actions/runs/33910972161)

## Verdict

**APPROVE**

The two prior F2 blockers are corrected at the audited tip. Bearer and login password failures now share one direct-peer limiter across every authenticated route class and return typed `429 rate_limited` responses. The locked Controller TLS/HTTP graph now uses patched `h2 0.4.16` and `rustls-webpki 0.103.13`. Fresh Rust 1.83 quality gates, all Controller targets, focused security and publication-race tests, frontend quality and browser gates, live HTTP authentication checks, dependency analysis, and exact-tip hosted CI pass. No unresolved Controller production-reachable advisory or integrity blocker was found.

## Prior Blocker Closure

### Bearer throttling and peer identity: CLOSED

- `AuthService` owns one `LoginLimiter` shared by `login` and `authenticate_bearer`; both record failed Argon2 verification against the connecting `IpAddr`, return `RateLimited` after five failures within five minutes, and clear that peer's failures after successful verification.
- Axum `ConnectInfo<SocketAddr>` supplies the peer address. Task and operations middleware obtain the same extension through `peer_ip`; `Forwarded`, `X-Forwarded-For`, and `X-Real-IP` are not trusted as client identity.
- Authorization parsing invokes Bearer verification only for valid UTF-8 with the exact `Bearer ` prefix. Malformed or non-Bearer authorization values fall through to cookie-session authentication rather than creating an alternate password path.
- Auth, task, and operations error adapters all preserve `AuthError::RateLimited` as HTTP 429 with a typed `rate_limited` code.
- A live Controller run accumulated failures across login, readiness, task listing, settings, and SSE. The sixth localhost failure returned typed 429. Spoofed forwarding headers did not move the budget; a request sourced from `127.0.0.2` received an independent 401; an existing cookie session remained valid; a correct Bearer credential succeeded and cleared the localhost failures; the next invalid request returned 401.
- Focused integration tests additionally pass for the shared login/Bearer budget and typed task-middleware 429 response.

### TLS and HTTP dependency advisories: CLOSED

- `Cargo.lock` pins `h2 0.4.16` and `rustls-webpki 0.103.13`, replacing vulnerable `0.4.13` and `0.103.9` resolutions.
- Feature-unified workspace inversion proves `h2 0.4.16` reaches `videnoa-controller` through Axum/Hyper and Reqwest, and `rustls-webpki 0.103.13` reaches it through Reqwest/Hyper-Rustls/Rustls. These are the production Controller paths that required remediation.
- `cargo deny check advisories` no longer reports the prior `h2` or `rustls-webpki` advisories. Remaining vulnerability advisories are `quick-xml 0.38.4` through Tauri desktop packaging and `time 0.3.45` through Tauri/desktop or `videnoa-core`; workspace feature-unified inverse trees do not connect those versions to `videnoa-controller`.
- Remaining unmaintained-crate reports likewise trace through GTK3/Tauri desktop dependencies. They are workspace risks for their owning products, not Controller normal/build dependency paths.

## Security and Integrity Dispositions

| Area | Result | Current-tip basis |
|---|---|---|
| Authentication and authorization | PASS | Shared direct-peer login/Bearer throttling, session fallback, exact Bearer parsing, route middleware, live cross-route verification, and typed 401/403/429 behavior pass. |
| Sessions and secrets | PASS | Argon2id password verification, random raw session/CSRF values, digest-only persistence, absolute/idle expiry, touch/rotation/revocation, password-hash fingerprint invalidation, and cookie protections remain enforced. Reviewed secret-scan matches are synthetic test/log fixtures, not usable credentials. |
| CSRF and CORS | PASS | Session mutations require same-origin and CSRF proof; Bearer requests do not rely on browser cookies; hostile-origin and preflight tests prove no permissive credentialed CORS response. |
| SQL, migrations, indexes, and CAS | PASS | Six migrations, transaction rollback, WAL/foreign keys/busy timeout, enum/relation constraints, optimistic versions, submission ownership, atomic reservation, snapshot pagination, and 20,000-row query-plan/load coverage pass. |
| Lifecycle and recovery | PASS | Persist-before-side-effect, exhaustive transition/recovery matrices, durable submission ownership, bounded retry, cancellation boundaries, restart takeover, malformed-task isolation, and no duplicate same-generation remote submission are covered and pass. |
| Input and temporary paths | PASS | Capability-retained roots, descriptor-relative/no-follow reopening, traversal/symlink/FIFO rejection, replacement detection, bounded streaming, exact length/hash checks, and owned cleanup pass adversarial tests. |
| Publication and no-clobber | PASS on Linux | Platform no-replace finalization, descriptor-relative staging, regular-file verification, racing destination preservation, duplicate finalizers, cross-filesystem publication, and staging-replacement rejection pass. Native Windows publication races remain a host-specific residual risk. |
| Panic, unsafe, and typed errors | PASS | Controller forbids unsafe code. The production `ConnectInfo` expectation is at the server-internal boundary that always installs `into_make_service_with_connect_info`; domain and HTTP failure paths otherwise use typed errors. No production `todo!` or `unimplemented!` was found. |
| Module size and maintainability | PASS | Reviewed production Controller modules remain within the established 250 pure-line ceiling; the largest relevant modules are approximately 200-241 pure lines. Remediation is localized and does not introduce parallel authentication or publication paths. |
| Frontend dependencies and behavior | PASS | `npm audit --json` reports zero vulnerabilities; lint, typecheck, unit tests, production build, and all 47 Chromium E2E scenarios pass. |
| Rust dependencies | PASS for Controller | Patched Controller-reachable HTTP/TLS versions are present and verified through feature-unified paths. Current advisory failures terminate in other workspace products. Sources pass. The repository has no `deny.toml`, so default `cargo deny check licenses` rejects all licenses and is not an enforceable project policy result. |
| Packaging and runtime boundary | PASS | Controller remains GPU-free and independent of `videnoa-core`, ORT, CUDA, cuDNN, TensorRT, and model content; exact-tip archive/image and non-root runtime jobs pass. |

## Verification Performed

- Provenance at audit start: `HEAD == origin/dev == 67620f5b45d54205b44861e3cc5d59e88724e998`, branch divergence `0 0`, and no pre-existing worktree change. A concurrent modification to `F4-release-regression.md` appeared later and was excluded from this report.
- Hosted run `33910972161`: exact `headSha`, completed `success`, and all 14 jobs passed, including Controller Rust, fault/load/security, frontend E2E, Linux/Windows archives, container/image checks, and legacy product regressions.
- `cargo +1.83.0 fmt --all -- --check`: PASS.
- `cargo +1.83.0 check -p videnoa-controller --all-targets --all-features`: PASS.
- `cargo +1.83.0 clippy -p videnoa-controller --all-targets --all-features -- -D warnings`: PASS.
- `cargo +1.83.0 test -p videnoa-controller --all-targets`: PASS for every target.
- Focused tests passed: `login_and_bearer_share_the_direct_peer_failure_budget`, `protected_task_middleware_returns_typed_rate_limit_response`, `staging_replacement_after_verification_is_never_accepted`, and `duplicate_publication_finalizers_preserve_exactly_one_final_artifact`.
- Controller web `npm ci --no-fund`, `npm run lint`, `npm run typecheck`, `npm test -- --run`, `npm run build`, and `npm run test:e2e`: PASS; all 47 Chromium scenarios passed.
- Live localhost HTTP verification: valid login 200; first five cross-route invalid password attempts 401; sixth invalid attempt 429 with typed `rate_limited`; alternate direct peer 401; cookie session during limiter state 200; valid Bearer 200 and cleared failures; following invalid Bearer 401.
- `npm audit --json`: zero vulnerabilities.
- `cargo deny check advisories`: expected workspace failure only for remaining desktop/core advisories; prior Controller-reachable `h2` and `rustls-webpki` advisory IDs are absent.
- `cargo deny check sources`: PASS.
- Feature-unified inverse trees explicitly connect patched `h2` and `rustls-webpki` to Controller and connect remaining vulnerable `quick-xml`/`time` versions only to other workspace products.
- Source review covered auth boundaries and services, limiter semantics, route middleware/error adapters, CORS/CSRF, sessions, persistence/migrations/CAS, lifecycle/recovery, path capabilities, publication finalization, tests, workflows, Dockerfile, lockfile, module size, unsafe/panic sites, and secret-storage patterns.

## Residual Risk and Non-Blocking Observations

- The limiter performs Argon2 verification before recording and classifying a failed attempt, and a correct credential is intentionally accepted and clears the budget. This avoids administrator lockout and meets the tested policy, but it does not eliminate CPU cost from continued invalid requests after the response threshold. Upstream connection/request-rate controls remain advisable for Internet-exposed deployments.
- A staging-file replacement racing between verification and no-replace rename can leave attacker-controlled replacement bytes occupying a previously absent final path while the task fails verification. The task is not marked complete and an existing destination is never overwritten. The actor model already requires write access inside the publication directory, which also permits direct creation of that final path, so this is retained as an operational cleanup/ambiguity risk rather than a crossed trust-boundary blocker.
- Generic secret-bearing filename patterns such as `*.pem`, `*.key`, and `credentials.json` are not comprehensively ignored. No tracked usable credential was found, but `.gitignore` hardening is recommended.
- Native Windows no-clobber race behavior is supported by implementation review and hosted archive/runtime smoke, while the descriptor/filesystem race suite executed locally on Linux.
- The workspace still carries desktop/core vulnerability and unmaintained-dependency advisories. They do not reach the Controller graph but should remain visible to the release reviews for those products.
- The repository lacks a configured Cargo license allow-list, making `cargo deny check licenses` fail by default even for standard permissive licenses. Establishing an explicit reviewed policy would turn that check into meaningful governance evidence.

VERDICT: APPROVE
