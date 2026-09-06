# Controller Async Password Verification

## Root Cause

Controller Bearer authentication previously loaded the password hash and ran Argon2 verification synchronously inside async middleware. Under constrained hosted scheduling, concurrent password checks occupied Tokio workers. SQLite idempotency preflight operations could complete internally but were not polled promptly enough to release their connections, so sibling requests reached the pool acquisition timeout and returned HTTP 500 before task creation.

## Durable Pattern

- Treat password hashing and verification as blocking CPU work.
- Keep hash-file loading in the same `spawn_blocking` closure because it is synchronous filesystem I/O adjacent to verification.
- Expose one async credential-verification boundary and use it from both login and Bearer paths.
- Convert `JoinError` into a typed authentication infrastructure error and map it exhaustively at every HTTP boundary.
- Preserve rate-limit accounting outside the blocking closure so limiter state remains keyed and mutated on the normal service path.

## Regression Shape

Use a barrier to release eight authenticated duplicate-intake requests simultaneously against a fixture with one SQLite connection and a 100 ms acquisition/busy bound. Assert exactly one `201 Created`, seven `200 OK` replays, one durable task, one idempotency row, and no orphan rows. Capture unexpected response bodies so a generic 500 retains its typed API evidence.

This fixture reproduces executor starvation without increasing timeouts or weakening exactly-once assertions.
