# Videnoa Controller Task 8

The Controller remote boundary is implemented under `crates/controller/src/remote/` as small typed modules for configuration, DTOs, transport, transfers, jobs, compatibility, caching, and path derivation.

Production rules established by Task 8:

- Use reqwest with `default-features = false`, rustls, JSON, and stream support. Controller's normal dependency tree must not include native TLS, Videnoa core, ORT, CUDA, cuDNN, or TensorRT.
- Parse external JSON once into typed DTOs under a configured byte limit. Do not expose response bodies or URL secrets through errors.
- Stream uploads and downloads with bounded chunks and independent request, connect, and body-stall timeouts.
- Treat remote workflow names as opaque values. Validate file API paths for safe URL construction, but do not normalize meaningful remote spelling such as `..`.
- Submit runs with `Idempotency-Key`; classify created versus replayed outcomes from the HTTP status and preserve typed conflicts.
- Determine compatibility from workflow interfaces first, then preset evidence. Required input interfaces must be Path-compatible.
- Cache compatibility against a monotonic clock with explicit TTL and health/restart/error invalidation.

Verification commands:

```bash
cargo fmt --all -- --check
cargo clippy -p videnoa-controller --all-targets --all-features -- -D warnings
cargo test -p videnoa-controller --all-targets --locked
rustup run 1.83.0 cargo test -p videnoa-controller --all-targets --locked
```

Reqwest's inactive QUIC lock graph must remain compatible with Cargo 1.83. The verified resolution uses reqwest `0.12.27`, quinn `0.11.9`, and quinn-proto `0.11.13`; newer quinn-proto releases pull `cpufeatures 0.3.1`, whose Edition 2024 manifest Cargo 1.83 cannot parse.
