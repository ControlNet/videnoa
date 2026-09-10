# Automatic submission confirmation retries

## Behavior

Submitting attempts now recover from a finished request that timed out, disconnected, stalled, returned HTTP 429, or returned a server error without restarting the controller. The same attempt, worker, idempotency identity, workflow, and input/output parameters are retained. Worker persistence remains necessary for deduplication.

The retry delay starts after request completion, uses `retry.initial_seconds`, doubles per failure, and caps at `retry.maximum_seconds`. It deliberately does not exhaust `retry.max_attempts`: uncertain acceptance may mean an existing running job, so the controller retains its compute slot and continues confirming. HTTP client rejection and malformed/conflicting responses retain their existing terminal/ambiguity handling.

## Persistence and concurrency

- A finished request releases its submission owner and writes retry count/deadline to task and attempt in one SQLite transaction.
- Release is fenced by attempt version, owner, submitting status, absence of remote job ID, and the task's current attempt number. Cancellation may have changed the task version, so this operation preserves the cancellation request instead of requiring the stale task version.
- Claim acquisition checks the durable deadline in SQL as well as before calling SQL. It clears the attempt deadline and restores ownership while the request is active. Cloned reconcilers cannot overlap requests; a new generation retains the existing restart recovery behavior.
- A successful receipt whose lifecycle write conflicts also schedules confirmation recovery, allowing a concurrent cancellation to finish rather than stranding the owned attempt.
- Successful processing transition clears task/attempt retry metadata. Scheduling emits task change notifications and a WARN with task/attempt IDs, retry count, deadline, and sanitized error text; submission identities and credentials are not logged.
- Existing columns are reused; no database migration or request-body change is required.
- Pausing scheduling blocks new submissions, while confirmation retries for already-submitting attempts and cancellation reconciliation remain available.

## Verification

The initial results below are historical. See [the follow-up verification](controller-submission-retry-verification-2026-09-10.md) for baseline comparison, storage evidence, and the corrected multi-worker request-count assertion.

Tests use the isolated mock worker; no production video jobs or services are mutated.

```bash
cargo test -p videnoa-controller --test task20 submission_
cargo test -p videnoa-controller --test task20 -- --test-threads=4
cargo test -p videnoa-controller --no-fail-fast -- --test-threads=1
cargo clippy -p videnoa-controller --all-targets -- -D warnings
git diff --check
```

Expected: automatic recovery without restart creates exactly one remote job; early retries are deferred even across generations; retry delay caps and continues past the transfer retry limit; active requests remain exclusive; cancellation and receipt conflicts recover; permanent rejection does not retry. The focused submission suite (9 tests) and strict all-target Clippy passed. Full controller regression was attempted with default, four-thread, and serial execution; intermittent SQLite `PoolTimedOut` errors also affected unchanged authentication fixtures. Serial execution did not eliminate these failures. The complete serial run finished with 561 passed, 21 failed, and 1 ignored; failures were predominantly database pool timeouts, with transfer checkpoint/completion timeouts as well. A separate four-thread task20 run finished with 32 passed and 5 failed (four pool timeouts and one downstream cleanup completion timeout); all eight submission ownership/retry tests passed in that run. Earlier failed cancellation checkpoint and health recovery cases passed on this repeat. These observations do not establish that every full-suite failure is unrelated to the patch. Do not report the complete suite as passing.

The 2026-09-10 production incident was diagnosed on controller 0.1.3. This source change still requires deployment of the rebuilt controller with its existing database. The old running process does not gain this behavior from a Git push. Worker code is unchanged.
