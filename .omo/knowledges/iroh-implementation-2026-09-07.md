# Iroh implementation and verification — 2026-09-07

## Scope and decisions

Implemented in the `feature/iroh` worktree after rebasing on dev. The worker embeds
an iroh endpoint; Controller embeds the loopback TCP adapter. Registration takes
an Endpoint ID and the existing worker password. No ticket, explicit relay URL,
external proxy process, or persisted loopback port is required. N0 defaults supply
address discovery and relays; separate-network and forced-relay acceptance still
requires an external test environment.

The worker's `[iroh] enabled` defaults to false. WebUI changes reconcile immediately;
manual file edits require restart. The protected `/api/iroh` status returns public
identity, running state and startup errors. Identity and exclusive ownership lock
live in the existing persistent data root. Unix files have mode 0600 and symlink
protection; corrupt keys are rejected without regeneration. Windows inherits data
root permissions and has not been executed in this environment.

Each CONNECT stream authenticates before opening the fixed loopback API socket.
The existing Argon2 authentication/cache/limiter is reused, keyed by authenticated
iroh peer ID rather than the loopback address. All current APIs are exposed,
including administration and WebSockets; static WebUI assets are excluded. Password
rotation preserves existing tunnels and upgraded WebSockets; HTTP requests and new
tunnels require the current password. Removal/offline reset persists disabled
state before deleting authentication state and closes active tunnels. Later password
setup does not reenable iroh. Controller credentials are updated manually.

## Implementation map

- `crates/transport`: persistent identity, N0 endpoints, bounded CONNECT parser,
  per-stream authorization, fixed target, cancellation, shared per-peer connection
  cache, loopback forwarding. Uses pinned iroh 1.1.0 and iroh-proxy-utils 0.3.0.
  The proxy adapter implements authenticated admission because the plain proxy
  example does not satisfy the worker-password requirement.
- Worker core: separate API-only router and runtime; configuration/password
  mutations share a settings lock. App and desktop start reconciliation and retain
  local HTTP service on an iroh startup failure.
- Controller: common remote client handles both transports, including upload and
  download streaming. Each client lease binds its immutable endpoint/password to
  an ephemeral listener; dropping the client cancels forwarding. The process shares
  a persistent iroh endpoint and peer connections. Authentication failures map to
  existing remote error types. Changing address or credential resets probe state.
- Migration 0013 avoids rebuilding the referenced worker table: canonical iroh URI
  remains in the existing unique address column; generated transport/Endpoint ID
  columns expose metadata. API JSON uses distinct transport/endpoint_id fields;
  HTTP JSON remains compatible. Existing task foreign keys stay intact.
- Both WebUIs retain existing components. Worker settings display/copy the ID and
  require a password to enable; Controller worker form supports HTTP or iroh and
  requires the saved worker credential.
- Worker Docker build uses a separate Node stage and checked prebuilt WebUI assets,
  replacing the old empty-source cache-warming scheme. Linux build jobs include
  CMake; CI runs shared transport tests. Controller remains independent of GPU/core.

## Verification evidence

Commands ran in this worktree, with the Rust target isolated from production:

```bash
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target RUST_TEST_THREADS=2 cargo test --locked --workspace --all-targets
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-core reset_disables_persisted_iroh --lib
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
(cd web && npm run lint && npm run build && npm test -- --run)
(cd controller-web && npm run lint && npm run build && npm test -- --run)
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-transport n0_endpoint_id_only_connection -- --ignored
docker build -f Dockerfile.controller -t videnoa-controller:iroh-dev .
bash scripts/check_controller_container.sh videnoa-controller:iroh-dev --all
docker build --target builder -t videnoa-worker-builder:iroh-dev .
docker run --rm videnoa-worker-builder:iroh-dev /build/target/release/videnoa --help
bash scripts/tests/controller_docs_test.sh
bash scripts/tests/package_controller_test.sh
bash scripts/tests/package_controller_windows_static_test.sh
bash scripts/tests/controller_archive_root_files_test.sh
node scripts/tests/validate_ci_release_workflows.test.mjs
```

Workspace tests passed: 1,215 passed, 0 failed, 12 ignored across 61 test binaries.
The subsequently added offline-reset identity test also passed. Web tests passed
192 tests; Controller Web passed 146. Lint/build and Clippy passed. Public N0
ID-only discovery/authenticated data transfer passed separately in 6.64 seconds.
Controller release image and all isolated container smoke checks passed, including
restart/session persistence, read-only workspace rejection, and external media paths.
Worker release builder image and its binary `--help` also passed; the complete
CUDA/ORT/TensorRT runtime image and GPU inference were not run.
Packaging/docs/workflow checks passed; Windows archive checks are static only.

All new credentials, identities, in-memory jobs and payloads in tests are explicitly
synthetic. The integration tests exercise actual TCP/QUIC, HTTP uploads/downloads,
API authentication and WebSocket frames; no GPU inference is claimed. Streaming
SSE is verified before the test origin completes its response. Invalid credentials
and arbitrary CONNECT targets cannot reach the test origin. Rotation keeps the
established stream working; removal closes TCP and WebSocket streams.

A preexisting global logging-capture test was flaky at default parallelism; the
complete rerun used RUST_TEST_THREADS=2 and passed. Lockfile changes also required
the existing GPU dependency contract to compare exact package names: substring
`ort ` incorrectly matched `videnoa-transport`. SQLx's own tree is checked for
unwanted database drivers/crypto rather than rejecting legitimate iroh identity
crypto in the overall Controller graph.

Secret-guard checked all changed/untracked files without printing matching content:
the only match was an explicit frontend test password. All common sensitive file
patterns are ignored, including `*.key`; `iroh.lock` is also ignored. No files were
staged, committed or pushed. No production service or other worktree data was used.

## Release acceptance remaining

Different NAT/CGNAT networks, forced-relay throughput, representative concurrent
video transfer/control latency, and actual Windows runtime execution remain
external acceptance checks. Existing application retry/idempotency remains in
force; there is no byte-offset transfer resume or tunnel-level event replay.
