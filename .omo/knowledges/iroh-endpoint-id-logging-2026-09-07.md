# Worker endpoint ID in local logs

- Worker identity is persisted as a 32-byte private key in `iroh.key` under the data directory. The public endpoint ID is derived from that key, not stored as readable text. Never print or publish the key file.
- Data directory precedence: `--data-dir`, then `VIDENOA_DATA_DIR`, then `./data`. Standard Worker Docker deployment uses `/app/data`, mounted from host `./data`.
- Successful iroh startup now emits an INFO event with message `iroh enabled` and public `endpoint_id`. This includes startup with iroh enabled and re-enabling it later. The already-running early return prevents repeated status/config reconciliation from logging the ID again.
- Failed startup and disabled iroh do not emit this success event. Disabling preserves the private key, so re-enabling retains the ID.
- Persistent application logs normally live in the data directory's `logs/` subdirectory. File logging can fall back to console-only if unavailable; INFO filtering must permit the event.

Read logs without accessing the WebUI:

```bash
rg 'iroh enabled' "${VIDENOA_DATA_DIR:-./data}/logs"
docker logs videnoa 2>&1 | rg 'iroh enabled'
```

For a CLI data-directory override, point the first command at that directory's `logs/` instead. A matching event contains the public endpoint ID for Controller registration.

Verification (with the repository's documented runtime library environment):

```bash
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-core --lib iroh_tests
rustfmt --check --edition 2021 crates/core/src/server/iroh.rs
git diff --check
```

Expected: all three lifecycle tests pass and both formatting checks exit zero. After deploying the updated binary, enable iroh with a Worker password configured and look for the INFO event.
