# Self-hosted iroh relays (v0.1.9, branch `feat/iroh-relays`)

## Design

- `videnoa_transport::Network` (crates/transport/src/network.rs):
  - empty relay list -> `presets::N0` (public relays + pkarr/DNS lookup),
    unchanged behaviour;
  - non-empty -> `presets::Minimal` + `RelayMode::custom(relays)`, no address
    lookup at all (nothing published to dns.iroh.link, nothing resolved);
  - `Client::tunnel` adds every configured relay as a dial hint
    (`EndpointAddr::with_addrs(TransportAddr::Relay)`), so Endpoint-ID-only
    registration keeps working without discovery.
  - URLs must be http/https; duplicates are dropped.
- Changing relays alone would not remove the public dependency (discovery);
  this is why self-hosted mode drops address lookup too. Consequence: both
  sides must list the same relays, otherwise they cannot find each other.
- Worker: `[iroh] relay_urls` in `config.toml` (serde default, omitted when
  empty). `PUT /api/config` validates it (400). `reconcile_iroh` restarts the
  endpoint (same identity) when the relay set differs from the running one,
  and reports an invalid file value in `GET /api/iroh` `error`.
- Worker WebUI bug fixed on the way: the iroh toggle rebuilt
  `iroh: { enabled }` and would have dropped `relay_urls` on save; now it
  spreads the existing object.
- Controller: `[iroh] relay_urls` in `controller.toml`, file-only and read at
  startup (`configure_iroh(root, relay_urls)` sets a process-wide
  `OnceLock<Network>`). `RawIrohConfig` is `deny_unknown_fields`, skipped on
  write when empty, validated through `Network::with_relays` (Schema error).
  Web Settings rebuilds `ControllerConfig` from the request: `build_config`
  now takes the current config and carries `iroh` (like the workspace roots),
  so rewriting `controller.toml` keeps the section.

## Testing

- `iroh = { version = "=1.1.0", features = ["test-utils"] }` as a transport
  dev-dependency gives `iroh::test_utils::run_relay_server()` (self-signed
  cert). `Network::with_insecure_test_relay_tls()` (cfg(test) only) sets
  `CaTlsConfig::insecure_skip_verify()`.
- `self_hosted_relay_connects_by_endpoint_id_without_public_services`: the
  client has no lookup and dials by ID only, so the first contact must go
  through the local relay.
- Worker restart test uses unreachable relays (`http://127.0.0.1:9`): binding
  succeeds without a reachable relay, so no network is needed.
- Local `cargo deny check` (0.20.2) reports licenses FAILED even on the
  baseline because `deny.toml` only configures advisories; CI runs only the
  advisories check, which passes.
