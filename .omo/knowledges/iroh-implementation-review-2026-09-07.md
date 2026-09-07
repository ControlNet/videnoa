# Iroh implementation review after authentication rebase

Baseline: feature/iroh at f1a62b8, based on dev 7754537.
This is a source-backed implementation proposal, not implemented functionality.

## Subsequent planning decisions and current baseline

During planning, the user explicitly selected ALL existing worker APIs, including
configuration and password management, without WebUI static assets. This
supersedes the minimal-route proposals below. Reuse the complete API router and
its existing authorization semantics behind the authenticated tunnel.

Before implementation the feature branch was rebased onto dev `ae801df`, yielding
HEAD `604f441`. Earlier source findings must be checked against this updated
baseline when implementing. The full implementation plan is recorded in the
conversation; this rebase does not implement that plan.

## Accepted scope

### Confirmed tunnel password requirement

The user explicitly requires the worker's existing service password to authorize
iroh TCP forwarding. Reuse the existing verifier and saved Controller credential;
do not add a second password, identity allowlist, or independent credential store.
The encrypted iroh transport handshake precedes application authentication. Gate
each new CONNECT/tunnel before opening the fixed worker API socket or forwarding
bytes; do not describe this as password authentication inside QUIC/TLS itself.
Retain existing HTTP authentication inside the tunnel.

Confirmed follow-up policy:

- The iroh enable switch is configurable through both the config file and the
  WebUI settings page. That page also displays the worker Endpoint ID as read-only
  runtime identity information. Never expose the private identity in config/API
  responses. The ID is derived from persisted identity, not a user-editable ID.
- An absent/disabled worker password prevents iroh forwarding even if the enable
  setting is true. Distinguish configured enablement from operational availability
  so the UI can explain that a password is required.
- Removing the password closes existing tunnels and denies new ones. Implement
  race-safe admission/revocation so an in-flight verification cannot reopen a
  tunnel after removal. The user subsequently clarified that removal also turns
  iroh off: persist enabled=false and stop the iroh service. Adding a password
  later must not silently re-enable it. Retain the persistent identity.
- Changing the password preserves established tunnels. New tunnel requests must
  use the new password. Existing HTTP authentication inside a tunnel still checks
  current credentials: old-password HTTP requests can fail while the underlying
  tunnel stays open. An already upgraded WebSocket is not forcibly disconnected
  solely because of password rotation.
- Recommended lifecycle behavior: disabling the iroh switch stops admission and
  closes tunnels while retaining identity; enabling starts it when a password is
  configured. Default enablement and file hot-reload semantics are not yet user
  decisions. “Default” refers only to an absent iroh config field in new or older
  configs, not an additional switch. WebUI settings should apply through the
  existing settings lifecycle.

Published local iroh-proxy-utils 0.3.0 source inspection found that upstream
authorization executes before TcpStream::connect, but downstream create_tunnel
accepts only EndpointAuthority and constructs a bare CONNECT request without a
credential/header argument. Authenticated CONNECT therefore needs a bounded
client-side extension/adapter; it is not an existing password configuration flag.
Review parsed-request logging for credential redaction when adding headers.
Core AuthService verification is currently private; expose a narrow reusable
verification operation with peer-aware limiting rather than duplicating Argon2
or calling a public login route. No transport implementation has been added yet.

- Users deploy Videnoa, copy its Endpoint ID (not a ticket), and register that
  ID in Controller. Reuse the existing worker password field and semantics.
- Use N0 default relays and address publication/lookup. Self-hosting is deferred.
- Persist each application's identity in its existing persistent data root.
- Use iroh-proxy-utils TCP forwarding while retaining HTTP API semantics.

## Proposed architecture

Deployment clarification: neither side needs a separately deployed proxy daemon.
The worker can expose a single embedded iroh Endpoint, but that Endpoint must
dispatch an application protocol handler; binding an Endpoint alone does not
serve the existing HTTP API. With proxy-utils downstream on Controller, the
worker needs the compatible upstream CONNECT handler, which can be embedded
and restricted to its fixed internal API target. It needs no Controller-style
local ingress listener for translating outgoing HTTP requests.

Alternatively, worker code can serve HTTP directly over accepted iroh streams
and avoid the internal TCP target socket. That requires a matching HTTP-over-
stream transport on Controller, or explicit handling of proxy-utils' CONNECT
handshake before handing the tunnel stream to Hyper. Bare HTTP on the worker
does not interoperate with a CONNECT-speaking downstream automatically. The
user is exploring this distinction; this does not yet replace the selected
proxy-utils first-phase approach.

Use a shared lightweight transport crate, with no GPU dependency, embedded in
the applications. It owns identity loading, Endpoint lifecycle, connection pools,
forwarding cancellation and bounded concurrency. Worker targets a fixed internal
loopback API listener; Controller maintains an internal loopback listener per
registered iroh worker. Users do not configure these transient ports. Disable
environment HTTP proxies for this local client path. Use proxy-utils' protocol
ALPN consistently rather than inventing an incompatible protocol identifier.

Store the real endpoint kind and canonical ID in Controller persistence, never
the local tunnel port. Preserve existing HTTP registrations and password
keep/replace/clear behavior. Plan a new migration after current migration 0012;
do not edit released migrations. Reject duplicate normalized iroh identities.
Mapping the same machine through both HTTP and iroh cannot be inferred from
unrelated addresses alone and must not silently merge distinct registrations.

All six production VidenoaClient construction sites must use a shared factory:
manual retry, health, recovery, upload, download, cleanup. Preserve authenticated
request helpers, no redirects, inactivity deadlines, streaming, idempotency and
error classification. Bind client/tunnel lifetime to the selected endpoint so an
old client cannot send credentials to a newly assigned loopback port.

## Authentication findings and design recommendations

### WebSocket and server-pushed events

TCP-mode CONNECT forwarding transports opaque bidirectional bytes, so WebSocket
upgrade/frames and streaming HTTP SSE are compatible in principle. This has not
yet been tested in Videnoa. Long-lived streams need suitable idle/keepalive policy
and must not inherit finite JSON request deadlines. Transport recovery does not
guarantee event replay; reconnect and state resynchronization remain application
responsibilities.

Current Controller WebUI SSE is `/api/events` between browser and Controller,
outside the proposed Controller-to-worker iroh path. Worker WebUI uses
`/api/jobs/{id}/ws` against window.location.host. Adding backend iroh connectivity
does not automatically route browser WebSockets over iroh. The earlier proposal
to omit that route from the minimal iroh router was an API scope proposal, not a
transport limitation; revisit it if worker WebSockets over iroh are required.
Current public WebSocket behavior must be distinguished from password-protected
HTTP APIs; do not claim that proxying adds authentication or immediate revocation
to an established socket.

Acceptance should cover upgrade, bidirectional frames, idle followed by push,
SSE incremental delivery, concurrent file transfers, relay paths, disconnect/
reconnect, and password changes with active connections. Existing tests contain
explicit synthetic fixtures; add real protocol tests with test-only payloads.

Sources: proxy-utils 0.3.0 crate protocol documentation; upstream main
src/downstream.rs TCP branch (implementation reference, not a pinned-version
test); controller operations/events.rs and worker web/src/api/client.ts.

The current worker intentionally permits open access when no password is set.
Health and job WebSocket routes are public. Controller sends credentials only
on protected requests and probes capabilities after public health. Preserve the
distinction between transport reachability and authenticated API readiness.

Recommend a dedicated minimal worker router for iroh using the SAME AuthService
and existing handlers. Include only Controller health, catalog, task and workspace
transfer operations; exclude browser login/password administration, general
filesystem browsing, config mutation and public task WebSockets. Raw TCP cannot
filter routes, so enforce this boundary at the target router. Constrain CONNECT
targets exactly and bound unauthenticated tunnel resources; do not treat HTTP
password verification as authorization to proxy arbitrary sockets.

The user confirmed a config/WebUI iroh switch and requiring an enabled worker
password before forwarding. Existing HTTP access retains its optional-password
policy. See the confirmed tunnel requirement above for removal versus rotation.

AuthService currently keys failed attempts by ConnectInfo<SocketAddr> IP.
Forwarding collapses remote identities onto loopback and thus shares limiter
buckets. Decide and test the boundary before shipping: a dedicated iroh limiter
or trusted internal propagation of verified peer identity can avoid interference
with LAN/browser clients. Do not trust arbitrary forwarded HTTP headers and do
not bypass the existing limiter. Correct cached Bearer credentials already skip
Argon2 and can succeed despite unrelated failed attempts; cache misses/startup
still need explicit coverage.

Identity creation must be atomic and private, reused after restart, and fail
clearly on corrupt existing files instead of silently generating a replacement.
Use the already resolved worker data_dir and Controller data_root; do not create
a second independently resolved persistent directory. Keep private files ignored
by git and never include them in config/API responses. Do not reuse one identity
file for concurrently active application instances.

## Implementation order and evidence

### Remaining first-release design choices

Recommended defaults, not yet user-confirmed:

- Disabled by default for existing deployments. WebUI changes apply immediately;
  manual config-file edits follow the existing application reload/restart contract
  rather than introducing a new watcher. Disabling closes tunnels but retains ID.
- Confirmed override: removing the password disables iroh and persists that state;
  later password creation does not automatically reactivate forwarding.
- Confirmed scope: iroh primarily serves worker APIs, not the worker WebUI. Do not
  add browser access or web publishing as a first-release requirement. The exact
  API route set remains an implementation contract to derive from required clients;
  this clarification alone does not authorize removing required API operations.
- Avoid opening arbitrary user-configured TCP destinations. The target is the
  application's internal worker API. Registration never asks for that port.

Engineering requirements requiring no extra user configuration: each CONNECT
stream on a pooled iroh connection must independently validate current credentials
so password rotation cannot authorize new tunnels through an old connection;
password removal must cancel all authorized streams despite concurrent handshakes.
Use peer EndpointId for tunnel admission limits plus global resource bounds.
Persist one long-lived Controller identity and avoid per-worker Endpoint creation.
Define client generation/lifetime so endpoint edits and port reuse never redirect
old credentials to a different worker. Keep permanent registration state separate
from transient direct/relay, connectivity and authorization status.

The user explicitly requires manual Controller credential updates after worker
password rotation, with no additional synchronization or password-update workflow.
Reuse existing errors and credential editing. Preserve bounded auth-failure blocking
and do not silently fall back to HTTP. Network reconnection does not imply task
replay or event replay; use existing recovery/idempotency semantics.

1. Shared identity/transport foundation: restart identity stability, malformed
   identity rejection, real loopback TCP forwarding, fixed-target rejection and
   shutdown/resource bounds.
2. Worker internal router and authentication: anonymous/wrong/correct password,
   rotation/removal on reused HTTP connections, public health versus protected
   catalog, excluded routes and shared-IP limiter behavior.
3. Controller endpoint model/migration/factory: existing HTTP rows reopen,
   password edits remain redacted, all six production paths choose the correct
   transport, tunnel replacement cannot cross worker identities.
4. Endpoint ID display and registration UI; explicit transport/auth/API errors.
5. Full task lifecycle with real isolated service instances, interrupted uploads
   and downloads, retry/recovery without duplicate jobs, packaging regressions.
6. Real separate NAT/CGNAT networks and forced relay fallback, measuring bulk
   throughput and responsiveness of control requests. Local checks cannot prove
   these acceptance criteria. No byte-offset resume is promised.

Use synthetic inputs only in explicitly identified test fixtures. Never access
the production service on port 3000 or its runtime files. Runtime setup for this
worktree must be checked independently before executing integration tests.

Existing regression commands to run after implementation and runtime setup:

```bash
cargo fmt --all -- --check
cargo test --locked -p videnoa-core server::auth --lib
cargo test --locked -p videnoa-controller --test worker_password --test task20
cargo test --locked -p videnoa-core -p videnoa-app -p videnoa-controller
cargo clippy --locked -p videnoa-core -p videnoa-app -p videnoa-controller --all-targets -- -D warnings
npm --prefix controller-web test
npm --prefix controller-web run lint
npm --prefix controller-web run build
```

Expected: no new failures, successful builds, and no lint warnings. Add focused
transport tests when the actual crate and test targets exist; verify optional
feature combinations and Linux/Windows/Docker packaging before shipping.
No application tests were run for this review-only documentation change.

## Evidence

- crates/core/src/server/mod.rs: app_router_with_static route/auth boundaries.
- crates/core/src/server/auth/mod.rs: RequestContext and failed-attempt limiter.
- crates/core/src/config.rs: data_dir resolution.
- crates/controller/src/main.rs: data_root and controller.sqlite3.
- crates/controller/src/remote/client.rs and transport.rs: sensitive per-request
  Bearer credentials, public health and disabled redirects.
- worker-bearer-auth-correction-2026-09-07.md: six client construction paths.
- https://docs.rs/iroh-proxy-utils/0.3.0/iroh_proxy_utils/
- https://docs.rs/iroh/1.1.0/iroh/endpoint/presets/struct.N0.html
