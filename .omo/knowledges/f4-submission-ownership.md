# Durable Submission Ownership

For Controller remote submission, a durable idempotency key prevents duplicate remote jobs but does not prevent duplicate HTTP requests. An accepted request with a lost response leaves the attempt in `Submitting`, so ordinary recovery can resend it.

The Controller now gives each `Reconciler` construction a generation identity and stores that identity on the attempt through a versioned SQLite claim immediately before `/api/run`.

- Cloned reconcilers share one generation identity.
- The owning generation defers later reconciliation of the same `Submitting` attempt.
- A new generation can replace the stored owner with a CAS and replay the unchanged submission key after restart.
- Scheduler admission must happen before claiming; otherwise a paused submission becomes owned without issuing its request and cannot resume in the same process.
- Successful submission evidence clears ownership as part of the transition to `Processing`.
- Submitting cancellation must pass through the same claim. The owning generation defers because it cannot prove whether its lost response represented acceptance; a new generation may replay the stable key and then reconcile cancellation.
- Migration proof must include a database stopped at the previous migration, not only reopening an already-current database. The 0005-to-0006 test verifies the nullable `TEXT` owner column and retained prior constraints.

This separates two guarantees: the ownership claim provides one request per process generation, while the unchanged remote idempotency key provides one remote job across legitimate restart replay.
