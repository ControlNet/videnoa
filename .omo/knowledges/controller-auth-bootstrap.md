# Controller Authentication Bootstrap

- The administrator credential is a singleton row (`id = 1`) in `administrator_credential`, introduced by migration `0008_administrator_credential.sql`.
- `Store::insert_administrator_credential` uses one `INSERT OR IGNORE`; callers must treat `false` as the setup race/conflict outcome and must never update the row.
- `GET /api/auth/setup` is public and returns `{ "initialized": boolean }`.
- `POST /api/auth/setup` accepts only `{ "password", "password_confirmation" }`, requires an exact Origin/Host match, and accepts matching passwords from 12 through 1024 bytes.
- Successful setup returns the existing `LoginResponse`, session cookie, and `x-csrf-token`; conflict is HTTP 409, invalid input HTTP 400, and origin failure HTTP 403.
- The former `hash-password` CLI is intentionally absent; first-admin setup occurs only through the guarded browser API and the CLI rejects that legacy subcommand without emitting PHC material.
- Argon2id hashing and verification run in `spawn_blocking` behind a two-permit semaphore. Each authentication/readiness decision reads the current SQLite PHC string and reuses the parsed credential only while that string remains unchanged.
- Durable sessions retain a credential fingerprint bound to secure-cookie issuance policy. Credential deletion closes readiness, credential rotation invalidates prior sessions, and enabling secure cookies rejects sessions issued under insecure-cookie policy.
- `AuthService::reconfigure(AuthConfig)` stays synchronous. Existing sessions are validated against `min(stored absolute expiry, created_at + current absolute lifetime)`, so tightened lifetime policy applies immediately without rewriting persisted rows.
- First client ownership remains a deployment risk when an uninitialized Controller is intentionally exposed beyond loopback; exact same-origin validation prevents cross-origin browser setup but cannot identify the intended first operator.

## Browser session lifetime (verified 2026-09-07)

- Default absolute lifetime is 2,592,000 seconds (30 days); idle lifetime is
  604,800 seconds (seven days), configured by `auth.session_absolute_seconds`
  and `auth.session_idle_seconds`. Deployed overrides may differ.
- Authenticated API requests, including the session endpoint, extend idle
  expiry to the earlier of now plus the idle duration or absolute expiry.
  The absolute deadline remains anchored to session creation.
- SSE periodic authentication uses `authenticate_passive` and does not extend
  idle expiry; merely keeping a stream open does not guarantee renewal.
- The HttpOnly cookie has Max-Age equal to the absolute lifetime, but the
  server also enforces idle expiry. Closing the browser alone does not revoke
  the session; clearing cookies, logout, or credential rotation can invalidate it.
- Sources: `config/raw.rs`, `auth/session.rs`, `auth/http.rs`, and
  `operations/events.rs` under `crates/controller/src`.

## Extended defaults (2026-09-07)

- Typed defaults and TOML defaults share the same session lifetime constants.
- Existing explicit TOML values are preserved. To adopt the new policy, set
  `auth.session_absolute_seconds = 2592000` and
  `auth.session_idle_seconds = 604800` in Web Settings, or edit the persisted
  configuration and restart. Log in again for the longer absolute lifetime.
- Historical SQLite migration defaults remain unchanged because those settings
  columns are no longer configuration authority.
- Regression tests cover default parsing, example configuration, serialization,
  cookie lifetime, idle renewal, passive checks, and exact expiration boundaries.
- Verification commands (all tests should pass; formatting and Clippy should
  exit successfully without warnings):

```sh
cargo test --locked -p videnoa-controller --test config_defaults_contract --test config_contract --test auth_http
cargo test --locked -p videnoa-controller --test config_bootstrap --test config_persistence --test auth_bootstrap --test auth_policy_reconfigure
cargo fmt --all -- --check
cargo clippy --locked -p videnoa-controller --all-targets -- -D warnings
```

## Settings session limit removal (2026-09-07)

- The original seven-day limit existed independently in Settings API validation,
  frontend request/response schemas, and HTML input max attributes. Changing only
  config defaults did not make the new policy editable through Settings.
- Session durations now have no fixed day cap in those layers. Positive integer
  and idle <= absolute checks remain; numeric representation limits still apply.
- Other request timeout and retry limits are unchanged.
- Tests use explicit test-only settings of 365-day absolute and 30-day idle
  lifetimes to exercise frontend save/refetch and backend API persistence.
- Verification (expect passing tests, lint/type checks, and successful build):

```sh
cargo test --locked -p videnoa-controller --test task14
cargo clippy --locked -p videnoa-controller --all-targets -- -D warnings
cargo fmt --all -- --check
npm --prefix controller-web test -- src/api/settingsSchemas.test.ts src/settings/SettingsPage.test.tsx
npm --prefix controller-web run lint
npm --prefix controller-web run build
```
