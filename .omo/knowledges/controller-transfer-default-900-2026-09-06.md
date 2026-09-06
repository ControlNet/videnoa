# Controller default transfer timeout: 900 seconds

- User requested increasing the default from 300 to 900 seconds (15 minutes).
- `config.rs` owns `TRANSFER_TIMEOUT_SECONDS = 15 * 60`; raw TOML initialization now references that same constant, preventing the two default paths from drifting.
- Updated the example TOML, Controller guides, and existing default-contract assertion.
- Existing persisted configurations and explicit custom values are preserved. An already-deployed NAS must set Transfer timeout seconds to 900 in Settings to adopt the value.
- Historical SQLite migration values remain untouched: they are not the active configuration source and editing applied migrations would break checksums. Test fixtures with explicit non-default values remain valid.
- `cargo +1.83.0 test --locked -p videnoa-controller --test config_defaults_contract --test config_bootstrap --test config_contract --test controller_docs`: 14 passed.
- Workspace fmt, strict all-target/all-feature Clippy, Controller documentation check, and diff whitespace check passed.
