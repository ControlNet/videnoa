# Videnoa Controller Task 6 Knowledge

## Harness Boundary

- The reusable mock lives only under `crates/controller/tests/support/mock_videnoa/` and is included by the single `tests/mock_videnoa.rs` integration-test crate.
- The server binds a real ephemeral `127.0.0.1` TCP listener. Restart and connection-refused modes stop and join the listener before rebinding the same address.
- `futures-util`, `hyper`, `hyper-util`, and `reqwest` are Controller dev-dependencies only; release dependency-tree and binary-symbol checks contain no mock fixture.

## Deterministic Transport Faults

- Disconnect-before-accept returns a Hyper service error before Axum routing, so request counters remain unchanged.
- Accept-then-drop persists the keyed run and journals acceptance before a private response header instructs the Hyper service to close the connection.
- Truncated downloads use an unknown-size body stream that emits the selected prefix and then an `UnexpectedEof`; a fixed-size body with a larger `Content-Length` makes Hyper panic before transport.
- Offline HTTP mode returns `503`; connection-offline mode drops the listener and produces a real connect failure at the unchanged URL.

## Checkpoints And State

- Named checkpoints use generation-bearing Tokio `watch` state, avoiding sleeps and lost notifications when reach or release precedes waiter polling.
- Persistent restart reloads JSON state, keeps files and idempotency mappings, and reconciles queued/running jobs to cancelled.
- State-loss restart returns `StateLostAmbiguous`; tests must not infer safe resubmission after durable evidence disappears.

## Journaling And Evidence

- Request journals record ordered sequences, methods, paths, byte-exact bodies, stable headers, route counters, logical checkpoint timestamps, response status, and typed outcomes.
- Volatile `Host` and sensitive authentication headers are redacted so evidence is byte-for-byte deterministic across ephemeral ports.
- Required evidence is generated at `.omo/evidence/videnoa-controller/task-6/mock-happy.json` and `.omo/evidence/videnoa-controller/task-6/mock-faults.txt`.
