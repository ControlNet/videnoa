# Controller Bearer Rate Limiting

- Controller login and Bearer authentication share one `LoginLimiter` owned by `AuthService`.
- The limiter key is the direct peer IP from Axum `ConnectInfo<SocketAddr>`. Forwarded headers are not trusted.
- `authenticate`, `authenticate_passive`, and `authorize_mutation` require the peer IP so every protected route uses the same policy.
- Order of operations (`auth/service.rs` `login`, `auth/session.rs` `authenticate_bearer`):
  1. `LoginLimiter::is_limited(peer, now)` runs first. A peer with five recorded failures inside the
     five-minute window receives the typed `rate_limited` response immediately; no credential is
     loaded and no Argon2 permit (`PasswordEngine`, two concurrent tasks) is consumed. Limited
     attempts are not recorded, so the budget frees when the oldest failure ages out of the window.
  2. Only unlimited peers reach Argon2 verification.
  3. A wrong password calls `record_failure`; a correct password calls `clear`.
- Consequence: once limited, even the correct password is rejected with 429 until the window
  passes. Before this ordering, the limiter only relabelled 401 as 429 after hashing, so a
  brute-forcer was never slowed and garbage Bearer tokens could queue legitimate logins behind the
  Argon2 semaphore.
- `AuthService::password_verification_count()` is a diagnostic counter of Argon2 verifications
  started; tests use it to prove a limited peer schedules no verification work.
- The limiter prunes peers whose window has expired on every recorded failure and caps the map at
  10,000 tracked peers by evicting the entry with the stalest latest failure.
- Cookie-session authentication is passive with respect to the password failure budget.
- Router unit and integration fixtures using `oneshot` must inject `ConnectInfo<SocketAddr>` because no TCP acceptor exists to add it automatically.
