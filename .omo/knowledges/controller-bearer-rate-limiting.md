# Controller Bearer Rate Limiting

- Controller login and Bearer authentication share one `LoginLimiter` owned by `AuthService`.
- The limiter key is the direct peer IP from Axum `ConnectInfo<SocketAddr>`. Forwarded headers are not trusted.
- `authenticate`, `authenticate_passive`, and `authorize_mutation` require the peer IP so every protected route uses the same policy.
- Five invalid password attempts within five minutes return unauthorized; the sixth returns the typed `rate_limited` response.
- Successful login or Bearer verification clears the peer's failures.
- Cookie-session authentication is passive with respect to the password failure budget.
- Router unit and integration fixtures using `oneshot` must inject `ConnectInfo<SocketAddr>` because no TCP acceptor exists to add it automatically.
