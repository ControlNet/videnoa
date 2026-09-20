# Controller Configurable Paths

## Public contract

- Settings exposes one `Paths` unit with `data_root` and `cache_root` only.
  Workspace, the derived configuration-file path, and readiness internals are not
  Settings fields.
- Public TOML uses `[paths] data_root` and `cache_root`. Internally `cache_root`
  continues to map to `PathConfig::temp_root` so task workspace code does not need
  a broad rename.
- The active configuration file is always `<DATA ROOT>/controller.toml`. Relative
  TOML paths resolve from the startup working directory; the Web API requires
  absolute paths.

## Runtime behavior

- A path change requires the scheduler to be paused and every task to be terminal.
  The change is persisted but does not replace live path capabilities.
- Settings reports `restart_required=true` while configured roots differ from
  active startup roots. Scheduler resume is rejected until paths are reverted or
  Controller restarts.
- CACHE ROOT is prepared during startup. Keeping it in a dedicated directory on
  the output filesystem lets publication take the atomic rename path.

## DATA ROOT activation and recovery

- Startup resolves DATA ROOT before opening SQLite. A move copies durable state
  to a new empty directory, syncs files and directories, marks the destination
  active, then updates the locator atomically.
- The default persistent root `<workspace>/data` owns
  `.videnoa-data-root.toml`, which locates a moved DATA ROOT after restart. Docker
  deployments must retain this mount even when DATA ROOT moves elsewhere.
- `.videnoa-data-root-migration.toml` records migration history. If the locator
  is lost, an active destination is reused without copying stale source state over
  it. Repeated moves preserve the full source chain.
- Prior roots are retained for rollback and remain private path boundaries, so a
  task cannot read or publish through directories that still contain Controller
  state. Nested source/destination roots are rejected before copying.

## Backup implication

- Back up the complete active DATA ROOT. When it differs from the default root,
  also back up `<workspace>/data` for its locator and retained rollback state.
- Back up CACHE ROOT only when transient task artifacts are needed for recovery.
