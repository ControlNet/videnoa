# Durable Submission Ownership

For Controller remote submission, a durable idempotency key prevents duplicate remote jobs but does not prevent duplicate HTTP requests. An accepted request with a lost response leaves the attempt in `Submitting`, so ordinary recovery can resend it.

The Controller now gives each `Reconciler` construction a generation identity and stores that identity on the attempt through a versioned SQLite claim immediately before `/api/run`.

- Cloned reconcilers share one generation identity.
- The owning generation defers concurrent reconciliation while its request is in flight.
- Since the 2026-09-10 automatic-retry fix, transient submission failures and receipt CAS conflicts release the finished request's claim and persist a retry deadline on both the task and attempt. The same generation can reclaim after that deadline, using the original idempotency identity and request body.
- Confirmation retries use the configured initial and maximum delays with capped exponential backoff. They do not exhaust the transfer retry limit: remote acceptance remains uncertain and compute capacity must stay occupied until reconciled.
- A new generation can replace the stored owner with a CAS and replay the unchanged submission key after restart, while respecting any persisted retry deadline.
- Scheduler admission must happen before claiming; otherwise a paused submission becomes owned without issuing its request and cannot resume in the same process.
- Successful submission evidence clears ownership and retry metadata as part of the transition to `Processing`.
- Submitting cancellation passes through the same claim and retry deadline. After a failed request finishes, the same generation may replay the stable key to identify and cancel accepted work.
- Migration proof must include a database stopped at the previous migration, not only reopening an already-current database. The 0005-to-0006 test verifies the nullable `TEXT` owner column and retained prior constraints.

This separates two guarantees: the ownership claim prevents overlapping requests within a generation, while the unchanged remote idempotency key provides one remote job across confirmation retries and restart replay.
