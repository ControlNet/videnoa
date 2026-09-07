# One-second Controller polling

The Controller orchestration cadence is now one second after each completed
stage/poll, independent of the remote request timeout (`timeouts.poll_seconds`,
still five seconds by default). Existing TOML files do not need modification.
SSE continues to publish changed progress immediately after persistence.

The recovery loop sleeps until the earlier of the next periodic scan or earliest
task deadline, removing the extra scan-cycle delay from the previous interval
plus cooldown combination. The active-task set still prevents overlapping polls;
durable retry and worker-health backoff still gate recovery. No frontend changes.

Verification passed:

```bash
cargo +1.83.0 fmt --all -- --check
cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.83.0 test --locked -p videnoa-controller --lib
cargo +1.83.0 test --locked -p videnoa-controller --test task20
bash scripts/tests/controller_docs_test.sh
```

19 unit tests and 31 orchestration/fault-recovery tests passed. Virtual-clock
coverage verifies the one-second deadline and no extra scan delay. The running
Docker container and existing images were not rebuilt/replaced in this change.
