# Disabled Worker iroh lifecycle correction (2026-09-07)

Supersedes the disabled-status behavior described in `worker-iroh-disabled-relay-investigation-2026-09-07.md`. The user explicitly requires disabled Workers to perform no iroh initialization, identity-file access, discovery, relay connection or retry work. Necessary configuration checks and shutdown cleanup remain.

## Changes

- `server/iroh.rs::status` now returns disabled/stopped with a null Endpoint ID and error immediately when configuration disables iroh. Status queries never initialize identity or transport, including when enabled.
- Only the enabled reconciliation path opens the persistent identity and starts the endpoint. The existing disabled endpoint-creation guard remains.
- Shared runtime shutdown cancels the private API listener, awaits transport shutdown and the API listener task, and releases the in-memory identity and its file lock. It clears stale startup errors. Existing identity files remain untouched for stable IDs on re-enable.
- The settings UI already supports a null Endpoint ID, leaving its field empty and copy action disabled. Its tests now reflect the revised contract; no frontend production code changed.
- This fixes confirmed disabled-state identity access. The previously reported relay log has not been reproduced while disabled, so it is not established that identity access caused relay traffic.

## Verification

Two new regression tests failed on the original implementation: querying disabled status created an identity, and querying it with an existing identity lock held produced an initialization error. Both pass after the fix. Tests use temporary directories and generated test identities, never production identity files.

The existing live lifecycle test now also disables via `PUT /api/config`, confirms an active tunnel closes, acquires the released identity lock, queries disabled status without initialization, re-enables and confirms the same Endpoint ID. Its password rotation, WebSocket preservation and password removal assertions continue to pass. This test was run once for the change; it is skipped in the subsequent wider suite to avoid repeated N0-dependent execution.

```sh
cargo test --locked -p videnoa-core --lib server::tests::iroh_tests
cargo test --locked -p videnoa-core --lib --tests -- --skip iroh_password_and_api_lifecycle
cargo fmt --all -- --check
cargo clippy --locked -p videnoa-core --all-targets --all-features -- -D warnings
npm --prefix web test -- src/pages/settings/__tests__/IrohSettings.test.tsx
npm --prefix web run lint
git diff --check
```

Focused Rust tests: 3 passed. Wider core suite: 626 passed, 10 ignored, 1 already-tested lifecycle test filtered out; integration tests: 3 passed. Frontend focused tests: 3 passed, including no identity while disabled and visible initialized identity while enabled. No remote configuration or running worker was changed, and no release binary was rebuilt or deployed for this fix.
