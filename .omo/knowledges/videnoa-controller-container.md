# Controller Container Contract

## Build

- Build `controller-web` in an isolated Node stage with `npm ci --no-fund` and `npm run build`.
- Copy only `controller-web/dist` into the Rust 1.83 stage and set `VIDENOA_CONTROLLER_WEB_PREBUILT=1`; `crates/controller/build.rs` still requires `dist/index.html`, then `rust-embed` places assets inside the release binary.
- Compile only `videnoa-controller` with `cargo build --release --locked -p videnoa-controller`. BuildKit caches npm, Cargo registry/git, and the Rust target directory.

## Runtime

- Runtime is Debian bookworm slim with CA certificates and curl, numeric UID/GID `10001:10001`, entrypoint `videnoa-controller`, and public liveness at `http://127.0.0.1:3001/api/health`.
- Persistent/configurable paths are `/etc/videnoa-controller`, `/var/lib/videnoa-controller`, `/var/tmp/videnoa-controller`, `/mnt/input`, and `/mnt/output`.
- The default command requires `/etc/videnoa-controller/controller.toml` and overrides only `--host 0.0.0.0`; local non-container defaults remain unchanged.
- The config must reference an Argon2id PHC file mounted separately, conventionally `/run/secrets/admin-password.phc`. Do not bake or record password, hash, Bearer, cookie, or CSRF values.
- A root-owned mode-0600 bind-mounted secret is unreadable to UID 10001. Use a secret mechanism or host ownership/mode that permits read access, while mounting it read-only.

## Verification

- `scripts/check_controller_container.sh videnoa-controller:qa --all` checks the Dockerfile contract, root Dockerfile preservation, image metadata, numeric user, Node/GPU/model absence, dynamic linkage, exported filesystem, CLI, embedded SPA, health, writable mounts, SQLite persistence, restart, and sanitized failure paths.
