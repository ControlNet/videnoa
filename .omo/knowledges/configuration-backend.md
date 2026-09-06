# Controller Configuration Backend

## Runtime Contract

- The controller has no runtime CLI configuration. It canonicalizes the current working directory as its workspace.
- Startup creates only `<workspace>/data`, with configuration at `data/controller.toml` and durable state at `data/controller.sqlite3`.
- Media input and output roots are the workspace. Data and temporary roots are `<workspace>/data`.
- The default listener is `127.0.0.1:3001`. Insecure cookies and non-loopback exposure produce startup warnings.

## Authority And Projection

- SQLite `controller_settings` is authoritative after initialization.
- `config_document` stores the authoritative TOML document; `pending_config_document` journals a projection that must reach disk.
- Updates use compare-and-swap versioning. A successful durable update is projected with atomic replacement and file/directory synchronization, then the pending journal is cleared.
- Startup repairs a pending projection before considering an offline TOML edit. A valid offline edit is imported into SQLite; malformed nonempty TOML is preserved and rejected.
- Bootstrap rejects symbolic links at `data/`, `data/controller.toml`, and the pending projection path. Atomic staging uses exclusive creation so pre-existing links cannot redirect writes outside the canonical workspace.

## Hot Application Order

- Settings validation produces typed server, auth, scheduler, timeout, and retry configuration.
- A changed listener address is prebound before the durable compare-and-swap update.
- After durable persistence and the first TOML projection attempt, scheduler and auth runtime policy are applied, then the listener is handed to the shared-router server loop.
- After a successful SQLite CAS, runtime policy and listener handoff are applied even if TOML projection fails. The API returns a retryable committed-degraded `503`, while a serialized version-checked background task repairs the projection and clears the pending journal automatically.
- A server-address change is rejected before persistence when the operations state has no listener capability, preventing test/helper states from claiming a false hot apply.
- Existing authentication sessions keep absolute expiry while the current idle policy applies when sessions are touched.

## Verification

```bash
cargo test -p videnoa-controller --test config_bootstrap --test config_contract --test config_defaults_contract --test config_listener --test config_persistence
cargo test -p videnoa-controller settings_update_persists_projects_and_hot_applies_every_public_field --lib
cargo clippy -p videnoa-controller --lib --bin videnoa-controller -- -D warnings
cargo build -p videnoa-controller --bin videnoa-controller
cargo fmt --all -- --check
```

Expected signals are all tests passing, zero Clippy warnings, a successful binary build, and no formatting diff. For manual QA, launch `target/debug/videnoa-controller` from a fresh temporary directory and verify `GET http://127.0.0.1:3001/api/health` returns `{"status":"ok"}` while `data/controller.toml` and `data/controller.sqlite3` are created.
