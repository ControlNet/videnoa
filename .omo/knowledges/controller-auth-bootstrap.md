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
