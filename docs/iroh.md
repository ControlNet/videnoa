# Iroh worker connections

Videnoa supports authenticated TCP tunnels over iroh for Controller-to-worker
API traffic. By default it uses N0 address discovery and the public N0 relays;
[self-hosted relays](#self-hosted-relays) replace both. It attempts direct
connectivity where available and can use relay paths.
Only the worker's Endpoint ID is needed for registration; tickets, public inbound
ports, and user-managed TCP proxy processes are not required.

## Enable on a worker

1. Set a service password in the existing WebUI access settings.
2. In Settings, enable **Iroh** and save the configuration.
3. Copy the read-only **Endpoint ID** shown in the same page.
4. In Controller, add a worker, select **Iroh**, and enter that ID and the worker
   password. Existing HTTP/HTTPS registrations continue to work.

The file equivalent is the following section in the worker's existing
`config.toml`. Restart the worker after editing the file. WebUI saves apply the
setting immediately.

```toml
[iroh]
enabled = true
```

The default is disabled. A worker without a configured password cannot accept
an iroh tunnel. Startup failures leave the local HTTP service available for
configuration; the iroh status reports the failure.

## Self-hosted relays

To avoid the public N0 infrastructure, run your own
[iroh relay](https://docs.iroh.computer/) and list it on **both** the Worker
and the Controller. With relays configured, an endpoint uses only those relays
and no public address lookup: it publishes nothing to `dns.iroh.link`, and the
Controller dials a worker's Endpoint ID through the configured relays. A
direct path is still attempted once the relay connects the two peers.

Worker, in its `config.toml` (restart after editing, or save Settings in the
WebUI to apply it):

```toml
[iroh]
enabled = true
relay_urls = ["https://relay.example.com"]
```

Controller, in `<DATA ROOT>/controller.toml` (restart after editing):

```toml
[iroh]
relay_urls = ["https://relay.example.com"]
```

- URLs must use `http` or `https`. An invalid value rejects the Worker
  settings save (400), shows as an error in the Worker's iroh status when it is
  in `config.toml` at startup, and stops Controller startup with a
  configuration error.
- The lists must match. A worker on self-hosted relays is not reachable from a
  Controller on the public network, and the reverse, because neither side
  publishes to or looks up the other's address service.
- Changing the Worker list restarts its endpoint with the same Endpoint ID.
  Settings saved from either WebUI keep the list; neither page edits it.
- Removing `relay_urls` (or setting `[]`) returns to the public N0 network.
- Put credentials for the relay host only in its own deployment, never in
  these URLs.

## Identity and authentication

Both applications store `iroh.key` and its ownership lock `iroh.lock` in their
existing persistent data directory. Keep this directory on the deployment's
persistent volume. Do not copy an identity into concurrently running instances.
Disabling iroh preserves its identity files but releases the in-memory identity
and file lock. While disabled, Worker does not initialize an endpoint, access
identity files, or run discovery/relay connections; status queries show no
Endpoint ID. Enabling it again loads the same identity. Invalid existing identity
files produce an error on enable rather than silently changing the Endpoint ID.

The key is private runtime state, excluded from git, and is not returned by APIs
or displayed in the UI. The Endpoint ID is public. Do not put passwords or key
contents into command arguments, config examples, URLs, logs, or tickets.

Each CONNECT stream authenticates with the worker's current service password
before the worker opens its fixed internal API socket. Reusing an existing iroh
connection does not bypass this check. HTTP authentication inside the tunnel
continues to use the existing service password behavior.

- **Change password:** established tunnels remain open; new tunnels require the
  new password. Protected HTTP requests also need the current password. Update
  the saved password manually in Controller. Established WebSockets are not
  disconnected merely because the password changed.
- **Remove/reset password:** iroh is disabled in persistent configuration and
  active tunnels are closed. Setting a password later does not turn it back on.
- **Disable iroh:** connections close while the stable identity is retained.
- Removing the password or disabling iroh through its own tunnel can close
  that request's connection before a complete HTTP response reaches the client.

## API contract

The tunnel exposes all existing worker APIs, including configuration, password
management, and job WebSockets. It does not serve static WebUI assets. It cannot
be used as a general-purpose proxy to arbitrary ports or other machines.

Controller worker create/update requests accept either the existing `api_url`
field for HTTP(S), or `transport: "iroh"` with `endpoint_id`. These endpoint fields
are mutually exclusive. The existing password create/keep/replace behavior is
retained; an iroh registration needs a saved password. HTTP responses retain the
legacy `api_url`; iroh responses return `transport` and `endpoint_id`.

`GET /api/iroh` on a worker returns `enabled`, `running`, `endpoint_id`, and
`error`. It uses the existing business API authentication. Running means the
local endpoint service started, not that another peer or the relay is reachable.

Migration 0013 retains existing worker IDs, task foreign keys, and the unique
canonical address column. HTTP addresses remain unchanged; iroh records store a
canonical `iroh://<endpoint-id>/` URI internally. Generated transport and ID
columns provide separate database metadata. Internal loopback ports are never
persisted or returned as worker identities.

## Long-lived connections and recovery

TCP mode transports WebSocket frames and SSE incrementally in both directions.
The backend tunnel does not automatically make a browser use iroh. Controller's
browser SSE remains on the normal browser-to-Controller HTTP path.

Existing streaming, inactivity deadlines, task idempotency, and recovery remain
in effect. An interrupted stream is not automatically resumed and missed events
are not replayed by the tunnel. Interrupted file transfers retain full-retry
behavior, without byte-offset resume. The implementation keeps application
memory bounded instead of buffering complete uploads/downloads.

## Verification

Run from the repository root using the existing Rust/native build environment:

```bash
cargo test --locked -p videnoa-transport
cargo test --locked -p videnoa-core iroh_password_and_api_lifecycle --lib
cargo test --locked -p videnoa-controller --test iroh_registration
cargo test --locked -p videnoa-controller controller_http_client_streams_through_authenticated_iroh --lib
cargo test --locked -p videnoa-transport n0_endpoint_id_only_connection -- --ignored
cargo test --locked -p videnoa-transport self_hosted_relay_connects_by_endpoint_id_without_public_services
```

Expected: all selected tests pass. The `n0_endpoint_id_only_connection` test
deliberately uses public N0 services; it is ignored in normal offline/local test
runs. The self-hosted relay test starts a local relay with a self-signed test
certificate and needs no network access. All test identities,
passwords, in-memory jobs, and streaming payloads are synthetic test fixtures.
No production service or runtime directory is used.

Local and single-host N0 tests do not establish separate-network NAT/CGNAT
compatibility, forced-relay throughput, or Windows execution. Those remain
release acceptance checks, together with representative concurrent video
transfers and control-request latency. Self-hosted relays are verified against
a local test relay only; deploying and operating a production relay is outside
Videnoa.
