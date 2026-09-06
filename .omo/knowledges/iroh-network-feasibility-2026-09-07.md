# Iroh connectivity feasibility

Research date: 2026-09-07. Repository baseline: `c3adfbf`.
Scope: source inspection and upstream documentation research; no transport was
implemented, compiled, or tested across NATs in this investigation.

## Finding

Videnoa can support iroh for Controller-to-worker traffic. Retain the existing
HTTP API contracts and add a transport beneath them. Start with an independently
built tunnel prototype to measure real network behavior; integrate into the
applications after choosing a toolchain policy. This is an engineering proposal,
not an existing Videnoa feature or a verified interoperability result.

HTTP is the application protocol; the current requirement for a reachable host
comes from the direct network connection. HTTP messages can travel over an iroh
bidirectional stream. Replacing HTTP with a custom RPC protocol is not required.

## Existing integration surface

- `crates/controller/src/remote/client.rs` owns a concrete `reqwest::Client` and
  `WorkerApiUrl`. It currently has no transport abstraction.
- `remote/transport.rs` constructs URLs, sends requests, bounds JSON responses,
  and classifies errors. Upload and download in `remote/transfer.rs` also call
  reqwest directly, so changing only `send()` would leave file transfers behind.
- `remote/jobs.rs` implements run, poll, and cancellation. Run submits a durable
  `idempotency-key` and distinguishes HTTP 201 from HTTP 200 replay.
- `remote/catalog.rs` retrieves workflows, presets, and workflow interfaces.
- Health probing, scheduling, remote cleanup, and recovery construct clients in
  multiple places. Native integration needs a shared endpoint/client factory
  across these paths, not a fresh iroh identity for every request.
- `domain/values.rs`, `domain/worker.rs`, persistence worker modules, and
  `controller-web/src/api/workerSchemas.ts` currently model HTTP(S) URLs only.
  The initial SQLite migration also makes `api_url` non-null and unique.
- `crates/core/src/server/mod.rs::app_router_with_static` provides existing Axum
  handlers. The router includes configuration, general filesystem, and other
  administrative routes beyond the Controller contract; do not publish this
  entire router to arbitrary peers.
- `crates/core/src/server/files.rs` streams files and confines transfer paths to
  the workspace. Upload truncates the destination; download does not implement
  HTTP Range. Existing transfers do not gain byte-offset resume from iroh.
- Upload timeout currently bounds the entire request; download separately bounds
  header/body waits. Slow relay uploads must be evaluated against this asymmetry.

## Follow-up: prioritize TCP forwarding with ecosystem components

The user selected TCP forwarding as the first-phase direction. Prefer evaluating
`n0-computer/iroh-proxy-utils` before implementing custom transport adaptation.
This updates the first-phase recommendation above; native HTTP adaptation is
optional future work rather than a required next step.

- [Repository](https://github.com/n0-computer/iroh-proxy-utils): maintained under
  the iroh team's organization, with a README explicitly marking its API as work
  in progress. It is a library with a CLI example, not a complete Videnoa daemon.
- [Downstream implementation](https://raw.githubusercontent.com/n0-computer/iroh-proxy-utils/main/src/downstream.rs):
  `ProxyMode::Tcp(EndpointAuthority)` opens an HTTP CONNECT tunnel inside an iroh
  bidirectional stream, then forwards opaque TCP bytes without parsing the
  application's requests. `DownstreamProxy` pools iroh connections and exposes
  `forward_tcp_listener` and `create_tunnel`.
- `UpstreamProxy` accepts the streams and connects to TCP origins; its pluggable
  `AuthHandler` must restrict both approved Controller identities and target
  authority. The [CLI example](https://raw.githubusercontent.com/n0-computer/iroh-proxy-utils/main/examples/cli.rs)
  uses `AcceptAll` and ephemeral endpoints; do not copy these defaults into a
  persistent service. Raw TCP forwarding cannot filter individual HTTP routes.
- The [0.3.0 manifest](https://docs.rs/crate/iroh-proxy-utils/0.3.0/source/Cargo.toml)
  declares Rust 1.85 but depends on `iroh = "1"`. That package declaration alone
  does not establish the resolved graph's MSRV: selecting iroh 1.1.0 still
  requires Rust 1.91. The [changelog](https://raw.githubusercontent.com/n0-computer/iroh-proxy-utils/main/CHANGELOG.md)
  dates 0.3.0 to June 15, 2026 and records migration to iroh 1.0.
- [Dumbpipe's README](https://raw.githubusercontent.com/n0-computer/dumbpipe/main/README.md)
  documents `listen-tcp --host` on the service side and `connect-tcp --addr` on
  the client side. It is a ready-to-run connectivity tool; this investigation
  did not establish its suitability for production authorization or supervision.

Proposed first phase: Controller uses a local TCP listener; an iroh-proxy-utils
downstream forwards to the approved worker endpoint; upstream connects only to
the configured worker API socket. Separate builds/processes preserve Videnoa's
current toolchain. For multiple workers, one local listener per worker is the
simplest mapping, with pooled iroh connections underneath. In containers,
loopback addresses apply to the containing network namespace, so use a shared
namespace or a private reachable service address as appropriate.

Existing HTTP URLs can point at the local listener, retaining reqwest, Axum,
headers, streaming, idempotency, and polling. HTTPS can pass through as bytes,
but origin hostname/SNI and certificate validation must still match; replacing
an HTTPS hostname with loopback does not automatically preserve verification.
No runtime or dependency compatibility tests were run in this follow-up.

## Verified upstream properties and constraints

[Iroh 1.1.0 crate documentation](https://docs.rs/iroh/1.1.0/iroh/)
describes QUIC streams, TLS peer authentication, relay-assisted establishment,
and subsequent direct connections where possible. Connecting requires a peer
EndpointId, addressing information (provided explicitly or resolved), and ALPN.
Authentication of a peer identity does not grant application authorization.

The [official FAQ](https://docs.iroh.computer/about/faq) documents outbound TCP 443
WebSocket relay connectivity, UDP direct connectivity, address lookup, self-hosted
relays, and public relay rate limits. A worker needs no public inbound port, but
still needs a reachable path out. Relay availability and UDP permission affect
whether the connection works and whether bulk data travels directly. An isolated
network with no usable path is not solved by iroh. Public infrastructure should
not be assumed to provide unlimited production video bandwidth.

[Iroh 1.1.0 was released on September 1, 2026](https://www.iroh.computer/blog/iroh-1-1-0)
and includes security fixes, notably relevant to relay operators.
Its [published manifest](https://docs.rs/crate/iroh/latest/source/Cargo.toml)
currently identifies version 1.1.0, edition 2024, and Rust 1.91 as the minimum.
The [version-tagged workspace manifest](https://raw.githubusercontent.com/n0-computer/iroh/v1.1.0/Cargo.toml)
provides a fixed source reference for the toolchain policy.

Videnoa declares Rust 1.83 in root `Cargo.toml`; `Dockerfile.controller` and
multiple unittest/release jobs pin that toolchain. Current iroh cannot simply be
added to that build. Native support needs a newer toolchain and compatibility
validation. An independent tunnel can retain the current application's build
baseline if built outside its workspace/dependency resolution. Merely adding a
feature flag does not prove Cargo 1.83 lockfile/resolution compatibility.

## Implementation choices

| Approach | Benefit | Cost / limitation |
| --- | --- | --- |
| Separate TCP-to-iroh tunnel at each end | Existing reqwest/Axum applications can stay unchanged; independent toolchain | Extra processes, local listeners, provisioning, and supervision; not native worker registration |
| Embedded HTTP over iroh bidirectional streams | Retains REST/JSON/status/body semantics and supports native endpoint registration | Requires stream IO adaptation, client/server connection lifecycle, authorization, and new toolchain |
| HTTP/3 over iroh | Existing ecosystem offers Axum adapters | Both client and server need compatible HTTP/3 integrations; existing reqwest configuration is not an iroh connector |
| Custom RPC plus separate blob transfer | Can evolve toward resumable content transfer | Reimplements contracts, error mapping, storage lifecycle, and recovery; unnecessary for initial connectivity |

[The official examples repository](https://github.com/n0-computer/iroh-examples)
includes HTTP forwarding with dumbpipe-web. It is a reference for the tunnel
pattern, not a reviewed production dependency.
[iroh-h3-axum 0.4.0 documentation](https://docs.rs/iroh-h3-axum/0.4.0/iroh_h3_axum/)
describes streaming Axum service over iroh HTTP/3. Its compatibility with iroh
1.1.0 and this project's dependency graph was not verified; do not select it
solely because the API concept matches.

## Recommended native design after the prototype

1. Keep browser-to-Controller HTTP(S). Add an explicit HTTP-or-iroh worker
   endpoint model, with migration for existing rows. Store worker EndpointId
   separately from mutable relay hints; reject duplicate identities. Decide how
   mixed HTTP/iroh registrations of the same physical worker avoid double slots.
2. Persist Controller and worker identities in private runtime storage so restarts
   preserve pairing. Keep private material outside git; if configured through
   environment files, use an ignored `.env.local`. Add controller allowlisting
   and revocation on workers; a public EndpointId or address ticket is not a
   password. Configure address lookup or explicit relay/address hints.
3. Reuse a long-lived Endpoint and pooled connections. Use a versioned application
   ALPN. One possible minimal adapter maps a bidirectional stream to HTTP/1.1 IO
   for Hyper and Axum. This is HTTP/1.1 over iroh, not standard HTTP/3.
4. Expose only required health, workflow/preset reads, run, job read/cancel, and
   workspace transfer methods to approved Controllers. A prototype tunnel must
   also restrict its fixed worker target and authenticate peers; do not make an
   unrestricted proxy. Local forwarding listeners require an appropriate local
   access boundary, including container network placement.
5. Preserve streaming backpressure, bounded memory, Content-Length validation,
   cancellation, error classification, idempotency, and recovery. Do not blindly
   replay a run after a lost response or downgrade to an unrelated HTTP target.
6. Surface transport type and direct/relay state alongside application health.
   Relay connectivity alone does not mean worker API readiness. Measure bulk
   transfer contention with job polling and enforce useful concurrency limits.

## Acceptance evidence required before shipping

- Two real networks behind NAT, including a CGNAT case: health, catalog, upload,
  run, poll, download, cancellation, and cleanup complete without port forwarding.
- Forced UDP blocking: relay fallback carries the same workflow; measure actual
  throughput, latency, memory, timeout behavior, and relay traffic/cost.
- Restart endpoints, change network paths, and interrupt relay access. Preserve
  identity; recover without duplicate computation. File interruptions must have
  documented full-retry behavior unless resume is separately implemented.
- Concurrent large file transfers keep control calls responsive. Verify bytes
  against local hashes and ensure incomplete results are never published.
- Unknown/revoked peers cannot invoke handlers. Workspace traversal protections
  remain effective through the new transport.
- Existing HTTP-only installations, database upgrade, Linux/Windows packaging,
  and Docker networking remain functional.

No such network or performance acceptance tests were run for this research.
Documentation-only checks for this change:

```bash
git diff --cached --check
python3 /home/zhixi/.codex/skills/secret-guard/scripts/scan_secrets.py staged
```

Expected: both exit zero, no whitespace errors or detected staged secrets.
