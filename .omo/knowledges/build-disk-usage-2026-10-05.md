# Build disk usage investigation (2026-10-05)

## Findings

- The checkout occupied approximately 443 GiB of allocated disk space; `target/` accounted for 436 GiB.
- `target/debug/` occupied 419 GiB: `deps/` 294 GiB, `incremental/` 114 GiB, `build/` 5.5 GiB, and `examples/` 4.8 GiB.
- `target/debug/deps/` contained 802 extensionless files occupying 251.84 GiB, with repeated hashed test executables including `task20`. The largest files were approximately 0.49 GiB each. Rust libraries and metadata accounted for most of the remaining dependency directory space.
- Other build trees included `target/release/` (12 GiB), `target/msrv-controller/` (5.5 GiB), and Linux/Windows target-specific outputs (approximately 1.2 GiB combined).
- The workspace manifest did not define custom development/test profiles. Accumulated test executables and incremental compilation artifacts explain the dominant disk usage; individual causes of each historical build variant were not reconstructed.
- `target/` is ignored and contains no Git-tracked files. Inspection found no Cargo/rustc build processes or accessible process executable, working-directory, or memory-map references to this checkout's `target/debug/` at the time of the audit.
- A live Videnoa process used a release executable whose original inode was already unlinked. Preserve release artifacts and avoid interrupting this process as part of debug cleanup.

## Cleanup recommendation

No build artifacts were deleted during this investigation. Before deleting, recheck that development builds, tests, and debug programs have stopped. Removing `target/debug/` is expected to reclaim approximately 419 GiB; the next debug build/test will regenerate required artifacts and take longer. Removing the separate `target/msrv-controller/` build tree can reclaim approximately another 5.5 GiB after checking it is unused.

Do not run a blanket `cargo clean` while retaining the current release installation. Preserve `lib/`, `bin/`, `models/`, `data/`, and `trt_cache/`: they contain runtime dependencies, media tools, models, persistent state, or costly engine caches. Preserve `.omo/` records and benchmark evidence unless their retention is separately reviewed.

Read-only verification commands, from the repository root:

```bash
du -x -h --max-depth=2 target
du -sh .
df -h .
git status --short --branch
```

Expected signals after a separately authorized debug-only cleanup: `target/debug/` absent, checkout size approximately 24 GiB, more filesystem free space, and unchanged tracked source files. Actual reclaimed space may differ due to concurrent writes or open file handles.

To limit future growth, consider disabling incremental compilation or reducing development/test debug information after assessing the build-speed and debugging tradeoffs. No build configuration was changed during this audit.
