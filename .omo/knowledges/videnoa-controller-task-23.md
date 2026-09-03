# Videnoa Controller Task 23

## Deterministic Standalone Archives

- Derive the release name from the workspace `Cargo.toml`, then require the
  executable's `--version` output to match before archive creation.
- GNU tar becomes repeatable when member order, ustar format, timestamp,
  owner/group, numeric ownership, modes, and gzip header metadata are normalized.
- .NET ZIP creation should add entries in explicit order and set every entry to
  the ZIP epoch (`1980-01-01T00:00:00Z`); ordinary `Compress-Archive` does not
  expose enough metadata control for a byte-repeatability contract.
- Verify archives against one complete ordered manifest rather than checking only
  required entries. This rejects loose frontend assets, models, runtimes, caches,
  secrets, existing Videnoa binaries, and every other accidental extra by default.

## Cross-Platform Proof Boundary

- A Linux host can prove the native tar layout, ELF architecture/linkage, CLI,
  embedded SPA, health endpoint, and checksum repeatability.
- A Windows archive must validate an `MZ` executable, execute `--version` on a
  Windows host, scan for forbidden GPU runtime DLL references, and normalize ZIP
  entry metadata. Static source checks on Linux are useful but are not native
  PowerShell or Windows runtime evidence.
