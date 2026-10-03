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
- Controller: `[iroh] relay_urls` in `controller.toml`, read at startup (`configure_iroh(root, relay_urls)` sets a process-wide
  `OnceLock<Network>`). `RawIrohConfig` is `deny_unknown_fields`, skipped on
  write when empty, validated through `Network::with_relays` (Schema error).
  Web Settings rebuilds `ControllerConfig` from the request: `build_config`
  now takes the current config and carries `iroh` (like the workspace roots),
  so rewriting `controller.toml` keeps the section.

## GUI editing (branch `feat/iroh-relay-settings-ui`)

- Controller API: `SettingsUpdateRequest.iroh: Option<IrohSettingsDto>`
  (omitted keeps the saved relays; `set_paused` sends `None`), response
  `iroh: IrohSettingsResponse { relay_urls, restart_required }`.
  `apply()` builds from the *committed* config (`config_manager().config()`),
  not the startup one, or an omitted `iroh` would revert a saved change.
- `iroh.restart_required` is deliberately separate from the top-level
  `restart_required`: controller-web disables the scheduler pause/resume
  button while the top-level flag is set (paths need a paused scheduler), and
  a relay change must not lock it. The page's "Restart required" pill and the
  save receipt use either flag.
- Validation: `Network::with_relays` -> `InvalidField("iroh", ...)` (400);
  controller-web mirrors it with a `new URL()` http/https refine.
- Worker WebUI: textarea in `IrohSettings.tsx` keeps a raw draft (so typing a
  newline is not swallowed) and re-syncs when the saved list or the form list
  changes (save / reset). An empty list is stored as an omitted `relay_urls`
  so the dirty check matches the Worker's `skip_serializing_if`.
- Testing-library: a `Field` whose label also holds a hint or error needs a
  regex label query (`/^Relay URLs/`).

## Multiple relays

- `relay_urls` may hold several relays (own + N0 public ones). Each endpoint
  homes on one reachable relay; `dial_address` hints every listed relay, so
  the Controller finds a worker homed on any relay in its list.
- Transport tests (`relay_tests`): `dialing_reaches_a_worker_on_any_listed_relay`
  (worker [A], controller [B, A]) and
  `an_unreachable_relay_falls_back_to_the_next_listed_one` (both
  [`http://127.0.0.1:9`, A]). Runtime failover after the home relay dies is
  iroh's net_report behaviour and is not covered by a Videnoa test.
- N0 relay URLs (iroh 1.1.0 `defaults::prod`): use1-1 / usw1-1 / euc1-1 /
  aps1-1 `.relay.n0.iroh.link`.
- Home relay choice (iroh `net_report::add_report_history_and_set_preferred_relay`):
  lowest recent latency among reachable relays, sticky unless the new one is
  better than 2/3 of the current latency. List order is NOT a priority, so
  "own relay first, public as standby" is not possible without custom logic.
- `use_public_relays` (Worker `config.toml`, Controller `controller.toml`,
  Settings API `iroh.use_public_relays`, a checkbox in both WebUIs):
  `Network::including_public_relays(bool)` appends
  `iroh::defaults::prod::default_relay_map().urls()`; no-op without
  self-hosted relays. Both configs expose `IrohConfig::network()` so startup,
  validation and the Worker reconcile build the same `Network`. The UIs send
  `false`/omit the flag when the list is empty (it is ignored then) so an
  unused flag does not cause a spurious restart_required or dirty form.

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

## Visual QA of the Settings pages (2026-10-03)

- Controller: a temporary spec in `controller-web/tests/e2e/` (removed after
  use) reuses `installOperationalReadRoutes` + an `/api/settings` override.
  The page scrolls in an inner container and has a sticky save bar, so
  element screenshots of the last section get covered; scroll every
  scrollable element to its end and take a viewport screenshot instead.
  Spec files there are type-checked by the `tsc -b` in the webServer build
  (e.g. `NodeListOf` is not iterable with the project lib settings).
- Worker: `npx vite --port 5179` in `web/` plus a Node Playwright script that
  imports `controller-web/node_modules/playwright`. Mock with a predicate
  `(url) => url.pathname.startsWith("/api/")`; the glob `**/api/**` also
  matches Vite's `/src/api/*.ts` modules and blanks the page. The language
  follows the browser context `locale` when no locale is stored.
- Findings fixed: disabled checkboxes need a dimmed label (both UIs), check
  rows need a minimum rather than fixed height so wrapped labels stay inside
  on 375px, and the checkbox must not flex-shrink.
