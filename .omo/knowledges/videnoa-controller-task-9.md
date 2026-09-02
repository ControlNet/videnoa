# Videnoa Controller Task 9

The durable lifecycle policy is centralized under `crates/controller/src/lifecycle/`. Callers use `LifecycleService`; raw task-only and attempt-only status mutation APIs are intentionally absent so legal policy and paired SQLite CAS cannot be bypassed.

Production rules established by Task 9:

- Treat task and current-attempt status as one durable fact. Normal advancement, failure, cancellation completion, and downstream retry update both rows in one transaction.
- Return `DurableAction` only after the database commit. Submission is special: remote job ID and remote paths bind in the same transaction that changes `submitting` to `processing` and authorizes polling.
- Processing retry is explicit and creates a fresh attempt ID and submission key only after typed terminal remote evidence and workspace-cleanup evidence match the failed attempt.
- Download, verification, publication, and cleanup retries resume the existing attempt and never repeat successful AI processing.
- `remote_state_ambiguous` and `publication_ambiguous` override mutable retry metadata and are always blocked.
- Cancellation persists intent before cleanup for active work. Publishing, remote cleanup, and all terminal states reject cancellation as conflict.
- The recovery classifier is exhaustive across all 14 `TaskStatus` variants; adding a status requires compiler-visible policy updates.
- Input/output paths remain task-owned immutable facts. A collision is not a retry mutation and requires a new task.

Verification commands:

```bash
cargo fmt --all -- --check
cargo clippy -p videnoa-controller --all-targets --all-features -- -D warnings
cargo test -p videnoa-controller --all-targets --all-features
cargo build -p videnoa-controller --all-features
rustup run 1.83.0 cargo test -p videnoa-controller --all-targets --locked
rustup run 1.83.0 cargo build -p videnoa-controller --all-features --locked
```

Rust 1.83 compiles and runs the full controller suite. The optional strict Clippy run under 1.83 reports lint-version differences in pre-existing modules; the repository's required active-toolchain strict Clippy gate passes without warnings.
