# Controller input scans and transfer inactivity correction

## Baseline and scope

- Started from `origin/dev` / `dev` at `7122b707a44b0af3b404574f1ffd2c2014d2c114`.
- Used the repository's pinned Rust 1.98.0. The requested historical 1.83.0
  verification commands predate the workspace MSRV/toolchain upgrade.
- This change only corrects Controller input scanning and transfer inactivity.
  It adds no migration, protocol, lifecycle state, dependencies, or frontend work.

## Why the content identity stays

Commit `5a091b3da3259f16aa4845107b51212afe11ddf3` addressed a real Task 12
regression: remove/recreate can reproduce device, inode, byte length, and mtime.
Those values cannot prove that bytes are unchanged. The durable nullable
`input_content_identity` remains the first 16 bytes of a full SHA-256 digest.
Legacy NULL rows intentionally retain metadata-only comparison behavior.

The older metadata/TOCTOU reopen design acquired full hashing after `open_input`
already performed it. That layering accidentally caused two full scans during
intake and two more before PUT. A second admission scan never made the subsequent
SQLite commit atomic with filesystem writes; upload-time verification is still
needed regardless of how many times intake hashes.

## One hash per phase

| Phase | Before | After |
| --- | --- | --- |
| Single-task intake / each batch-created file | 2 full hashes | 1 full hash |
| Fresh upload admission | 2 full hashes | 1 full hash |
| HTTP PUT body | 1 complete read | 1 complete read |
| Total normal new-task path | 5 full-file reads | 3 full-file reads |
| Batch preview | 0 hashes | 0 hashes |

`RootedInput` retains the descriptor hashed by `open_input`. Hashing remains
capability-based, validates a regular file, records platform identity/size/mtime,
rechecks descriptor metadata after the complete SHA-256 pass, and rewinds it.
`revalidate_metadata` checks the retained root, the retained descriptor, and the
current no-follow capability path without scanning contents. Intake uses that
cheap revalidation before persistence.

`upload_input::open_verified` hashes once and compares the snapshot against the
durable metadata and optional content identity. `into_verified_file` then cheaply
revalidates and returns that same rewound descriptor for PUT. The old explicit
`reopen_checked` API retains its full-verification semantics for existing callers;
neither production intake nor upload calls it anymore.

The regression suite covers changed bytes with identical metadata, same-size
remove/recreate, zero PUT on mismatch, legacy metadata-only acceptance, retained
root and symlink rejection, and byte-zero transfer. A per-path counter compiled
only under `cfg(test)` measures real single/batch service intake and the production
upload admission helper. No test instrumentation is present in production builds.

## Transfer timeout history and correction

Commit `db1c7eb971bff301e8ec33b1932e73ab4e1e496b` fixed the short poll timeout
incorrectly applying to transfers. Downloads became per-header/per-chunk bounded,
but uploads received a total request deadline. Commit
`ad8c229c668c9954219b6f059a04f3b8ea66d9ed` raised the default from 300 to 900
seconds. The whole-upload deadline and advice to configure it above complete
upload duration are now superseded.

- Connect: maximum connection/TLS establishment time, unchanged reqwest connect
  timeout. Runtime settings currently map `health_seconds` here.
- Request: total deadline for short control requests and their response bodies,
  unchanged `timeouts.request` / runtime `poll_seconds`.
- Transfer/stall: inactivity bound, retaining `transfer_seconds` and default 900.
  A progressing upload has no total-duration deadline. Fifteen minutes now means
  fifteen minutes without observable activity.

A bounded `ReaderStream` emits each upload chunk. Every non-empty successful chunk
updates a `watch` channel with its actual monotonic timestamp. The send future is
polled alongside a deadline based on the latest timestamp, so delayed watchdog
polling cannot manufacture progress. Updates coalesce in constant memory.
Backpressure eventually stops chunk emission, allowing the watchdog to expire.
EOF/channel closure preserves the last deadline while awaiting response headers;
receipt JSON retains existing bounded-size/per-chunk response handling.

Watchdog expiry returns `Stall`. A separate local-read error flag preserves
`LocalIo`; connection/transport/status classification and transient retry policy
remain unchanged. Downloads retain independent header and body-chunk waits;
control requests retain their total request deadline.

## Limitations

- Synchronous task/batch creation still awaits one complete hash per file and can
  take noticeable time on large NAS media. Batch creation remains sequential.
- Upload verification is still a synchronous full-file operation in the existing
  executor path; this patch removes its duplicate scan, not its scheduling model.
- Retained descriptors prevent path replacement from changing the uploaded handle;
  they are not immutable filesystem snapshots or locks against concurrent in-place
  writes after verification. The existing requirement to keep admitted media stable
  remains; no extra atomicity claim is made.
- Observable upload progress means handing bytes to HTTP transport, not a Worker
  acknowledgement or durable disk write. Transport/OS buffering can delay detection
  of a remote read stall until backpressure stops body polling.
- Timing tests use synthetic bytes and real local TCP; they are not a NAS throughput
  benchmark or a multi-hour production media transfer.

## Verification

Run Cargo checks serially because the integration suites use process/network
fixtures. All listed commands should succeed; Clippy must report no warnings.

```sh
cargo +1.98.0 test --locked -p videnoa-controller --lib input_cost_tests
cargo +1.98.0 test --locked -p videnoa-controller --test task12
cargo +1.98.0 test --locked -p videnoa-controller --test videnoa_client
cargo +1.98.0 test --locked -p videnoa-controller --test task_api --test path_capabilities --test workspace_paths
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.98.0 test --locked -p videnoa-controller --all-targets
bash scripts/tests/controller_docs_test.sh
```

Focused results: input-cost tests 2 passed; Task 12 25 passed; remote client 24
passed; task/batch API 36 passed; path capabilities 9 passed; workspace paths 8
passed. Clippy and docs checks passed. Changed Rust modules were additionally
formatted directly because the library's `include!("module_topology.rs")` means
workspace cargo-fmt does not traverse all source modules.

Real TCP activity tests use 250 ms inactivity with 60 chunks paced at 50 ms:
upload and download each last roughly three seconds (over 10 times the timeout)
and succeed. Mid-body reader inactivity, network backpressure, and response-header
inactivity after EOF return `Stall`. Local reader failure returns `LocalIo`.
The existing held control response still returns `Timeout` at the request deadline.

## Changed files

- `.omo/knowledges/controller-batch-intake-cost-2026-09-06.md`
- `.omo/knowledges/controller-input-content-identity.md`
- `.omo/knowledges/controller-input-scans-transfer-inactivity-2026-09-07.md`
- `.omo/knowledges/controller-nas-transfer-timeout-2026-09-06.md`
- `.omo/notepads/videnoa-controller/learnings.md`
- `crates/controller/src/paths/input.rs`
- `crates/controller/src/paths/input_hash_counts.rs`
- `crates/controller/src/paths/input_identity.rs`
- `crates/controller/src/paths/mod.rs`
- `crates/controller/src/remote/config.rs`
- `crates/controller/src/remote/mod.rs`
- `crates/controller/src/remote/transfer.rs`
- `crates/controller/src/remote/upload.rs`
- `crates/controller/src/scheduler/mod.rs`
- `crates/controller/src/scheduler/upload_fresh.rs`
- `crates/controller/src/scheduler/upload_input.rs`
- `crates/controller/src/tasks/input_cost_tests.rs`
- `crates/controller/src/tasks/intake.rs`
- `crates/controller/src/tasks/mod.rs`
- `crates/controller/tests/path_capabilities.rs`
- `crates/controller/tests/task12/upload.rs`
- `crates/controller/tests/videnoa_client.rs`
- `crates/controller/tests/videnoa_client/upload_activity.rs`
- `docs/controller.md`

Full Controller suite: 534 passed, 0 failed, 1 ignored
across 49 test harnesses; command exited 0. The ignored test is the
existing opt-in production Argon2 contention stress test. Validation ran on Linux;
no Windows execution or real NAS throughput benchmark was performed.
