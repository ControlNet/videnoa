# Videnoa Controller Task 10

Startup recovery is owned by `crates/controller/src/recovery/` and remains separate from future scheduler selection.

- `Store::recovery_tasks` and `Store::current_attempt` make SQLite the only restart authority.
- Recovery reuses the durable submission key, binds accepted remote evidence through `LifecycleService`, and polls only the persisted remote job ID.
- Missing persisted remote jobs become nonretryable `remote_state_ambiguous`; restart-cancelled jobs become retryable processing failures requiring explicit retry.
- Worker health failures persist bounded exponential backoff while retaining task assignment and used capacity.
- `ShutdownCoordinator` closes stage intake, persists scheduler pause, and bounds the drain of tracked durable writes without cancelling remote compute.
- The controller binary reconciles before serving and handles both SIGINT and SIGTERM through the same pause-and-drain path.
- Recovery-only failure commits may omit an unusable attempt snapshot, but ordinary lifecycle failure still requires one outside queued state.
- Cancellation intent must branch before ordinary recovery dispatch: submitting uses accepted/not-accepted keyed reconciliation, while processing cancels the known job before terminal cancellation.
- Remote processing identity is the persisted job ID, workflow name, and exact input/output params; any contradiction fails closed as nonretryable ambiguity.
- Worker health retry counts cap at the configured bound, clear the next check at exhaustion, and retain task assignment/capacity.
- Every durable recovery mutation holds a `WritePermit`, including lifecycle transitions, ambiguity failures, and worker-health updates.

Verification:

```bash
cargo fmt --all -- --check
cargo clippy -p videnoa-controller --all-targets --all-features -- -D warnings
cargo test -p videnoa-controller --all-targets --all-features
cargo test -p videnoa-controller --test mock_videnoa -- --test-threads=1
cargo build -p videnoa-controller
cargo +1.83.0 test -p videnoa-controller --test mock_videnoa -- --test-threads=1
```
