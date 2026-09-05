# F2 Code Quality, Security, and Data-Integrity Review

Audit date: 2026-09-05

Audited tip: `30d9f25d19cf0ec1a88733483da7f95581e980ad`

Hosted authority: [GitHub Actions run 33946244764](https://github.com/ControlNet/videnoa/actions/runs/33946244764)

## Verdict

**APPROVE**

The exact pushed tip resolves the observed authentication executor starvation without weakening authentication, session, CSRF, or idempotency contracts. No release-blocking code-quality, security, or data-integrity defect was found in the nine-file change.

## Authentication Boundary

- `PasswordFile::verify` now clones its `PathBuf` and takes an owned `SecretString` into `tokio::task::spawn_blocking`, keeping hash-file loading and Argon2 verification off the asynchronous executor.
- Login and Bearer authentication both await this shared verification boundary; neither retains a synchronous Argon2 call on the request executor.
- Blocking-task join failure maps to typed `AuthError::PasswordVerification`, and all task, operations, readiness, and authentication HTTP adapters exhaustively map that internal failure without exposing credentials or implementation detail.
- Invalid credentials still produce `Unauthorized` or `RateLimited`. Successful login, Bearer authentication, session issuance, password-fingerprint invalidation, cookie attributes, and CSRF behavior are unchanged.
- The shared direct-peer failure budget remains in force for login and Bearer requests.

## Concurrent Idempotent Intake

- The regression uses a nine-party barrier to release eight authenticated duplicate requests together against a one-connection SQLite pool with a 100 ms acquisition timeout.
- The exact test passed three consecutive local runs. Each run produced one creator response, seven replay responses, and one durable task rather than the former authentication-induced `Database(PoolTimedOut)` HTTP 500.
- The broader intake race passed ten repetitions with sixteen contenders per repetition and reported ten durable tasks total, proving one durable request body/task identity per idempotency key.
- Production idempotency remains transactionally enforced: `task_idempotency.idempotency_key` is the primary key, and task plus idempotency insertion share one transaction. Conflict and replay rollback paths do not leave orphan candidate tasks.

## Security and Quality Review

- No changed-file `unsafe`, panic, `unwrap(`, `expect(`, direct print, or credential-logging hazard was found.
- Secret Guard's tracked scan found no real credential. Its two matches are deliberate synthetic fixtures for unknown-field rejection and split-write redaction.
- The Controller normal/build dependency graph contains no `videnoa-core`, ONNX Runtime, CUDA, cuDNN, or TensorRT path.
- `npm audit --audit-level=low` reported zero vulnerabilities, and `cargo deny check sources` passed.
- LSP diagnostics reported no findings for all nine changed Rust files; `git diff ... --check` passed.
- Production changed files remain below the 250 pure-LOC ceiling; `crates/controller/src/auth/service.rs` is 236 pure LOC.

## Verification Performed

- Provenance: `HEAD == origin/dev == 30d9f25d19cf0ec1a88733483da7f95581e980ad`.
- Hosted run `33946244764`: exact `headSha`, conclusion `success`, all 14 jobs successful.
- `cargo +1.83.0 fmt --all -- --check`: PASS.
- Strict Controller Clippy with `-D warnings`: PASS.
- Focused `auth_http`, `persistence_atomic`, `persistence_migrations`, and `task_api` tests: PASS.
- `cargo +1.83.0 test --locked -p videnoa-controller --test task21 intake_race::mixed_duplicate_intake_races_preserve_one_request_body -- --exact --nocapture`: PASS, ten repetitions and sixteen contenders per repetition.
- `cargo +1.83.0 test --locked -p videnoa-controller --test task_api concurrency::concurrent_duplicate_intake_creates_exactly_one_task -- --exact`: PASS in three consecutive runs.

## Non-Blocking Residual Risks

- Rate limiting records failures after Argon2 verification, so it bounds authentication responses rather than admission to blocking work. This behavior predates the fix and does not weaken the existing direct-peer failure budget, but a separate bounded admission control would provide additional denial-of-service hardening.
- Secret Guard reports 19 generic sensitive-file patterns not covered by `.gitignore`; no tracked real secret was found.
- Workspace-wide `cargo deny check advisories` and `cargo deny check licenses` remain blocked by pre-existing desktop/core dependencies and the repository-wide license allow-list configuration. Those failures are outside the Controller normal/build dependency graph.

VERDICT: APPROVE
