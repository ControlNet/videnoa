# Controller zero-config distribution contract

## Runtime and filesystem

- Run `videnoa-controller` from the workspace current directory.
- First start creates `./data/controller.toml` and `./data/controller.sqlite3`.
- SQLite sidecars and transient per-task UUID directories are allowed only under
  `data`; no generic input, output, config, secret, or auth directories are
  prepared.
- Task media paths are task-defined within the workspace. Relative paths resolve
  from the workspace, while the complete `data` subtree is excluded from task
  input, output, and recovery capabilities.
- Raw TOML sections are exactly `server`, `auth`, `scheduler`, `timeouts`, and
  `retry`. There is no paths section or password credential path.

## Authentication and settings

- `GET /api/auth/setup` reports `initialized`; first setup posts matching
  `password` and `password_confirmation` with an exact same-origin `Origin`.
- Passwords require at least 12 bytes. Only the Argon2id hash is stored in
  SQLite. Setup returns the normal login cookie and CSRF response; repeat setup
  conflicts.
- Settings GET returns read-only `workspace`, `data_root`, and `config_file`
  metadata, server settings, and scalar auth policy.
- Settings PUT sends `version` plus complete `server`, `auth`, `scheduler`,
  `timeouts`, and `retry`. Accepted changes persist and hot-apply, including the
  listener and auth policy.

## Container and archive

- `Dockerfile.controller` uses `/workspace`, preserves `USER 10001:10001`,
  declares no legacy volumes, and explicitly passes `--host 0.0.0.0` for
  container networking.
- Operators bind one writable common-parent host workspace to `/workspace` and
  should use `--user "$(id -u):$(id -g)"` to avoid root-owned host files.
- Bind-all startup must remain loopback-published for trusted first setup or be
  protected by firewalling and same-origin HTTPS.
- Controller archives still contain only the executable, example TOML, archive
  README, and license. Existing deterministic layout/version/GPU exclusion
  checks remain valid.

## Verification on 2026-09-05

- `bash scripts/tests/controller_docs_test.sh`: passed.
- `bash scripts/tests/controller_archive_root_files_test.sh`: passed.
- `bash scripts/tests/package_controller_test.sh`: passed using a synthetic C
  executable fixture; this is not a production binary build.
- `bash scripts/tests/package_controller_windows_static_test.sh`: passed static
  checks; native Windows execution was not available.
- `bash scripts/check_controller_container.sh videnoa-controller:qa --source`:
  passed Dockerfile source checks.
- `cargo +1.83.0 test --locked -p videnoa-controller --test controller_docs
  --test contract_evidence --no-fail-fast`: blocked before these tests by
  concurrent peer-owned compile errors in `operations/settings.rs` and
  `recovery/shutdown.rs`. Do not attribute those errors to distribution files.
- Full Docker build/runtime setup/restart smoke was deferred to lead integration
  because the current Controller binary cannot compile until peer changes land.
