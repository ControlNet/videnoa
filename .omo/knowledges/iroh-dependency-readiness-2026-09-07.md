# Dependency readiness for iroh TCP forwarding

Baseline: `5ffeb99`, Rust 1.98.0. No production manifest, dependency lock, or
implementation changes were made during this investigation.

## Method and result

Copied the real workspace manifests, Cargo.lock, toolchain file, and all crate
sources into `/tmp/videnoa-iroh-deps-4qd2l_is`. Added exact `iroh = "=1.1.0"`
and `iroh-proxy-utils = "=0.3.0"` workspace dependencies only in that copy,
consumed by both core and Controller with their default features. No placeholder
source implementations were used. Cargo resolved the graph against the copied
existing lockfile without a blanket `cargo update`.

Both real library targets passed `cargo check` with the added dependencies on
Linux/Rust 1.98.0 (68 seconds). This verifies dependency resolution and compilation
of the candidate libraries alongside existing code. It does not exercise proxy
API usage, NAT, TLS initialization, Windows, Docker, or the full test suite.

Reproduction against the retained temporary copy, using the existing conda
`anime` native runtime configuration from AGENTS.md:

```bash
audit_dir=/tmp/videnoa-iroh-deps-4qd2l_is
CARGO_TARGET_DIR=/tmp/videnoa-iroh-audit-target cargo check --locked \
  --manifest-path "$audit_dir/Cargo.toml" \
  -p videnoa-controller -p videnoa-core --lib
cargo tree --locked --manifest-path "$audit_dir/Cargo.toml" \
  -p videnoa-controller -i pem-rfc7468@1.0.0
```

Metadata and diagnostics are in `/tmp/videnoa-iroh-audit-metadata.json`,
`/tmp/videnoa-iroh-audit-resolve.log`, and `/tmp/videnoa-iroh-audit-check.log`.
These temporary paths are investigation artifacts, not project configuration.

## Existing direct dependencies need no prerequisite upgrade

The current lock already satisfies the shared requirements of the selected
iroh/proxy versions:

| Dependency | Existing locked version |
| --- | --- |
| tokio | 1.49.0 |
| tokio-util | 0.7.18 |
| hyper | 1.8.1 |
| hyper-util | 0.1.20 |
| http | 1.4.0 |
| http-body-util | 0.1.3 |
| bytes | 1.11.1 |
| rustls | 0.23.36 |
| tokio-rustls | 0.26.4 |
| tracing | 0.1.44 |

Existing reqwest 0.12.27 continues to compile alongside 0.13.2 used by iroh.
Version 0.13.2 was already present in Cargo.lock, but `cargo tree -i
reqwest@0.13.2` had no active dependents in the original Linux graph. Do not
confuse lockfile presence with an already enabled production dependency.

Do not migrate the application HTTP client merely to match iroh's reqwest.
No dependency-driven reason was found to update Axum, SQLx/rusqlite, Tauri,
ort/ndarray, GPU libraries, or frontend packages for TCP forwarding. Keep the
existing HTTP and runtime contracts and the shared SQLite binding alignment.
Do not upgrade the application's rand_core 0.6 or sha2 0.10 to match newer
versions used by iroh; Cargo resolves these as separate major-version families.

## Transitive lockfile changes actually observed

These are the versions Cargo selected, not universally minimal required targets.
Requirements below come from the resolved packages' published manifests.

| Package | Existing | Selected with iroh | Reason in this resolved graph |
| --- | --- | --- | --- |
| zeroize | 1.8.2 | 1.9.0 | iroh-base 1.1.0 requires ^1.9 |
| bitflags (2.x) | 2.10.0 | 2.13.1 | simple-dns 0.12.0 requires ^2.11 |
| ipnet | 2.11.0 | 2.12.2 | netdev 0.45.1 / 0.46.2 require ^2.12 |
| data-encoding | 2.10.0 | 2.11.1 | selected data-encoding-macro family requires ^2.11.1 |
| plist | 1.8.0 | 1.10.1 | netdev 0.46.2 requires ^1.10 |
| indexmap (2.x) | 2.13.0 | 2.14.2 | plist 1.10.1 requires ^2.14.0 |
| time | 0.3.45 | 0.3.55 | plist 1.10.1 requires ^0.3.47 |

Related selected replacements include hashbrown 0.16.1 -> 0.17.1,
quick-xml 0.38.4 -> 0.42.0, deranged 0.5.6 -> 0.5.8,
num-conv 0.1.0 -> 0.2.2, time-core 0.1.7 -> 0.1.9, and
time-macros 0.2.25 -> 0.2.32. Some are platform-specific lock metadata rather
than active Linux code. The graph also adds iroh/QUIC/address-discovery/crypto
packages and additional incompatible version families. This is not evidence
that every existing dependency must be updated.

Recommendation: let a bounded lockfile update accompany the feature dependency
addition, then review and test that diff. Avoid adding unused direct dependencies
or upgrading the entire workspace solely to pre-stage these transitive changes.

## Integration issues beyond version numbers

1. **Crypto features:** iroh defaults enable `tls-ring`, while proxy-utils enables
   reqwest 0.13's `rustls` feature, activating AWS-LC. The resolved graph contains
   both ring and aws-lc-rs (plus aws-lc-sys). Current reqwest versions explicitly
   select their fallback provider, so dual features alone do not demonstrate a
   runtime panic. Still verify endpoint/client initialization and native build
   tools on each supported platform. Disabling default features on our direct
   iroh dependency cannot subtract features enabled by proxy-utils transitively.
2. **Dependency contract test:** Controller's
   `dependency_tree_activates_only_sqlite_sqlx_driver` currently forbids
   `pem-rfc7468` anywhere in the Controller dependency tree. The candidate graph
   legitimately introduces it through iroh -> ed25519-dalek -> pkcs8 -> der.
   When adding the feature, scope this assertion to the SQLx subtree while
   preserving the MySQL/Postgres and GPU exclusion guarantees. Do not try to
   remove legitimate identity cryptography just to satisfy the old global test.
3. **Feature isolation:** keep iroh optional if HTTP-only builds should avoid
   the new networking/native crypto footprint. A shared transport crate can
   centralize compatible iroh types for worker and Controller when implemented.

## Upstream references

- [Proxy-utils manifest](https://raw.githubusercontent.com/n0-computer/iroh-proxy-utils/main/Cargo.toml)
- [Iroh 1.1.0 manifest](https://raw.githubusercontent.com/n0-computer/iroh/v1.1.0/iroh/Cargo.toml)
- [Proxy-utils repository and API status](https://github.com/n0-computer/iroh-proxy-utils)

Version-specific dependency requirements were also verified against Cargo's
downloaded published manifests, not inferred from version numbers alone.
