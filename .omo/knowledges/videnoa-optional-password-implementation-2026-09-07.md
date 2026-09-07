# Optional Service Password Implementation

## Production isolation

- The existing Videnoa service on port 3000 is production. Never request, attach to,
  stop, restart, signal, or debug that instance during development. Do not modify
  its data, configuration, binaries, or frontend build output.
- This work uses branch `feat/optional-password` in the isolated worktree
  `/tmp/videnoa-password-WyWeBE9F/worktree`, with a separate Cargo target directory
  and fresh runtime fixtures. Only test instances on loopback ports 13000/13001
  or random allocated ports are used. No production deployment is included.
- Vite proxies now default to 13000 and 13001, with explicit development URL
  overrides. Controller preview continues to disable its proxy.

## Behavior and boundaries

- Missing credentials mean open access. WebUI Settings enables, changes, or removes
  the password without restarting the service or asking for the old password.
- `GET /api/health` and `/api/jobs/{id}/ws` remain public. Real socket regression
  coverage verifies anonymous subscription and continued messages after password
  rotation and removal. The socket test uses an explicitly test-only in-memory
  job without starting inference.
- Other business APIs accept a session cookie or `Authorization: Bearer <password>`.
  Explicit bad Authorization never falls back to cookies. Browser mutation paths
  require matching Origin and CSRF; standalone Bearer clients do not.
- Service cookies are named `videnoa_service_session`, intentionally different
  from Controller's existing `videnoa_session`. Cookies are scoped by host, not
  TCP port; reusing Controller's name broke concurrent browser sessions.
- Passwords allow 1–1024 UTF-8 bytes, preserve spaces, and reject control characters.
  Non-ASCII Bearer values are parsed as UTF-8 bytes rather than HeaderValue::to_str.
- The auth service uses private `auth.sqlite3` with Argon2id credentials and hashed
  session tokens, independent of optional job persistence. Startup/storage errors
  fail closed. Password rotation and session replacement are transactional.
- `auth.lock` provides exclusive ownership through std File locking. The reset CLI
  runs before application logging/runtime initialization and touches only the
  explicitly resolved authentication directory.
- Session defaults are 30 days absolute and seven days idle, both configurable
  through existing config/Settings. Shortening applies to existing sessions;
  extending absolute duration requires login again. Expired sessions are removed.
- Secure-cookie policy is configurable, defaults false for LAN HTTP, and changes
  invalidate existing sessions. Passwords/hashes are never part of config responses.

## Controller worker credentials

- Worker responses expose only `has_password`. Create supports an optional
  password; update uses missing=keep, string=replace, null=clear. Empty strings
  are rejected rather than silently interpreted as removal.
- Migration 0011 adds a nullable retrievable password to the private Controller
  database. Plain retrievable storage was explicitly selected by the user;
  Controller needs the original credential for outbound Bearer authentication.
- SecretString provides redacted Debug output. Credential edits are part of the
  existing worker-version transaction and wake health/capability refresh.
- All five production client construction paths use the saved credential.
  Reqwest default sensitive headers cover even direct upload/download send calls.
  Redirects remain disabled. No anonymous fallback occurs after auth failure.

## Verification notes

- Frontend suites cover login boundaries, CSRF/401 handling, open-access behavior,
  and Worker credential keep/replace/clear semantics; all credentials are test-only.
- Isolated browser/API smoke covers mobile and desktop login, session Settings
  persistence, logout, both Worker dialogs, protected files, a real StringTemplate
  task without GPU inference, worker discovery through Controller, and removal.
- Existing FIFO security coverage hit its one-second deadline during a heavily
  parallel initial run, then passed alone and in the reduced-concurrency full
  Controller run without modifying the deadline or assertion.
- The old idempotency restart fixture retained the previous AppState while opening
  another one. It now consumes the old state and waits for both executor release
  and the actual file lock before reopening. Arc strong-count zero alone is not
  sufficient because field destructors can still be running.

Use the project environment from AGENTS.md and isolated build output:

```bash
cd /tmp/videnoa-password-WyWeBE9F/worktree
export CARGO_TARGET_DIR=/tmp/videnoa-password-WyWeBE9F/target
export CARGO_BUILD_JOBS=2
export RUST_TEST_THREADS=2
cargo test --locked -p videnoa-core -p videnoa-app -p videnoa-controller
cargo clippy --locked -p videnoa-core -p videnoa-app -p videnoa-controller --all-targets -- -D warnings
cargo fmt --all -- --check
PKG_CONFIG_PATH="$HOME/miniconda3/envs/anime/lib/pkgconfig:$PKG_CONFIG_PATH" cargo check --locked -p videnoa-desktop
npm --prefix web test
npm --prefix web run lint
npm --prefix web run build
npm --prefix controller-web test
npm --prefix controller-web run lint
npm --prefix controller-web run build
```

Expected: passing tests, no lint/Clippy/format errors, and successful builds.
Frontend bundle-size and third-party annotation notices are non-failing warnings.

## Final results

- Core: 621 unit tests passed; its 10 existing model/media integration tests remain
  ignored. App: 22 unit tests passed; core crash-hook integration tests also passed.
- Controller unit and integration suites passed in the reduced-concurrency run.
- WebUI: 190 tests passed. Controller WebUI: 144 tests passed.
- Auth-specific regressions, doctests, Clippy, formatting, both frontend builds,
  debug service builds, and desktop compilation passed.
- Final browser smoke passed after checking the mobile Worker dialog for overflow.
  All spawned smoke services were stopped; production port 3000 was never queried.
