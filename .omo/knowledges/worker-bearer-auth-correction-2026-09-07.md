# Worker Bearer authentication correction

Starting origin/dev: 92f000486d94e1888092a7c6eb9830a0f6e959fa.
Rebased onto concurrent diagnostics fix 221aaf7f4acc6d365d5da02498dfbf0610f6fa62.
Rust toolchain and MSRV: 1.98.0, from rust-toolchain.toml and Cargo.toml.

## Production audit

All six Worker client construction sites use new_with_password with the saved
Worker credential:

- operations/tasks.rs: processing_retry (fixed missing credential).
- workers/health/probe.rs: probe.
- recovery/reconciler.rs: reconcile_task.
- scheduler/upload.rs: upload (including stat and partial cleanup).
- scheduler/download.rs: download (including stat).
- scheduler/cleanup_remote.rs: delete_remote_workspace.

All protected control requests in remote/catalog.rs, remote/jobs.rs and
remote/transfer.rs use send_authenticated. Upload and download preserve their
custom inactivity deadlines and use the same authenticated request-builder helper.
Only remote/client.rs health uses send_public directly. The reqwest client has no
default Authorization header; the optional stored HeaderValue is sensitive.
Redirects stay disabled. Anonymous new remains available for deliberate tests and
utilities, with a source architecture regression guarding production constructors
and direct reqwest client construction outside remote.

## Runtime state

AuthService owns an ephemeral Option<[u8; 32]> under its existing database mutex.
Successful Bearer validation caches SHA-256(password). Cache hits compare with
subtle::ConstantTimeEq and do not run Argon2. Cache misses retain the existing
Argon2id verifier, password validation and failed-attempt limiter. A correct cached
credential can authenticate even after bad attempts from its peer; bad Bearer never
falls back to cookie authentication. Browser login continues its existing verifier.

Committed password creation/rotation populates/replaces the cache. Disable clears
it. Startup starts empty. Offline reset requires the instance lock, preventing a
live stale cache from surviving reset. SQLite credential schema remains id and
salted Argon2id password_hash only; no fast verifier, plaintext Worker password,
new database, migration or machine-session protocol was introduced.

WorkerHealthService holds an in-memory WorkerId -> version authentication block.
401/403 capability failures go offline with the existing saved-password error and
stop periodic probes. Store health updates increment version, so the block uses
expected_version + 1 from the successful CAS, not a possibly newer reloaded record.
Registration edits bypass the block and old deadline immediately; deleted records
are pruned. Controller restart discards blocks. Other failures keep retry/backoff.

## Verification

Worktree: /tmp/videnoa-auth-fix. Build artifacts:
/tmp/videnoa-password-WyWeBE9F/target. No production service requests or process
operations. Temporary mock servers and real smoke instances are test-only.

```bash
cd /tmp/videnoa-auth-fix
export CARGO_TARGET_DIR=/tmp/videnoa-password-WyWeBE9F/target
export CARGO_BUILD_JOBS=2
export RUST_TEST_THREADS=2
cargo test --locked -p videnoa-core server::auth --lib
cargo test --locked -p videnoa-controller --test worker_password --test task20
cargo test --locked -p videnoa-core
cargo test --locked -p videnoa-controller
cargo clippy --locked -p videnoa-core -p videnoa-controller --all-targets -- -D warnings
cargo fmt --all -- --check
npm --prefix web test
npm --prefix web run lint
npm --prefix web run build
npm --prefix controller-web test
npm --prefix controller-web run lint
npm --prefix controller-web run build
```

Expected: zero failures; existing model/media-dependent core tests stay ignored.
Focused core auth: 9 passed. Core full: 622 unit tests and 3 crash-hook tests passed,
10 existing ignored tests. Focused Controller: 4 password tests and 33 Task 20 tests
passed, including protected manual processing retry to a completed replacement
attempt, protected upload/download/cleanup, single-failure blocking, password edit,
403 vs 429/503 behavior, and public health without Authorization. Frontends: 190
Web and 145 Controller Web tests passed; both lint/build passed. The existing Web
bundle-size advisory remains nonfatal.

Optional SecretString Serialize removal was deferred: Worker request DTOs and
existing request contract round-trip fixtures use it. Debug redaction, explicit
expose(), and response-only has_password behavior remain unchanged.

Final-base Clippy passed with -D warnings; fmt passed. Both isolated app/controller
debug builds passed. Existing real HTTP/browser smoke passed via
`node /tmp/videnoa-auth-browser-smoke.cjs` with the AGENTS.md runtime library
environment. This copy points to the new worktree and used temporary ports 33173
and 54811. It covered file transfer, task submission, protected Worker discovery,
desktop/mobile login, session settings persistence, logout, Worker dialogs, and
password disable. Both owned smoke processes exited and their ports were released.

Final Controller full suite: 557 tests passed across 51 test groups, zero
failures and zero ignored tests. Final secrets scan and .gitignore audit passed.
