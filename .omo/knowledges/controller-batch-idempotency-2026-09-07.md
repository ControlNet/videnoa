# Durable batch HTTP idempotency

## Contract

- `POST /api/tasks/batch` accepts an optional `Idempotency-Key` with the individual task endpoint's existing validation (one header, 1–255 visible ASCII bytes).
- No header preserves independent, sequential batch intake. Batch keys have their own namespace, separate from individual task keys.
- A typed, serialized batch request is SHA-256 fingerprinted. JSON property order and omitted-versus-null optional fields do not change the typed fingerprint. The pattern and all options participate; a changed request under an admitted key returns 409.
- First successful admission returns 201; replay returns 200 and the saved response snapshot. Partial admission returns and replays 207. Failed rows in a stored partial result are not retried automatically.
- Replays occur before filesystem preview, so membership and task IDs survive input removal, new matches, output creation, and Controller restart. Query task detail for live task status.
- Preview errors/no matches/invalid options still create zero tasks and do not consume the key.

## Persistence and concurrency

Migration 0010 adds `batch_idempotency`. For keyed batches, preview and file identity preparation happen before acquiring the database write transaction. A write-first unique-key claim serializes concurrent callers; the winner inserts tasks and stores the response in the same transaction. Per-row savepoints preserve the existing partial-admission result contract. No pending receipt is intentionally committed.

Receipt persistence failure or cancellation rolls back the transaction, so there is no committed task/receipt gap. Task creation logs and SSE events are emitted only after commit. A second receipt lookup after preview handles a competing request that completed while files were scanned; the transaction claim rechecks again after preparation. No in-memory lock or retry key is required for correctness across database connections or reopened service instances.

Stored receipts include response paths already present in task records and remain in the private Controller database. Logs do not expose the supplied key or receipt body. Batch response diagnostics retain bounded error summaries.

## Verification

Tests use temporary databases, synthetic media bytes, and explicitly test-only SQLite failure triggers. They cover eight concurrent keyed requests, service/database reopen with changed filesystem membership, different-payload conflict, reusable keys after preview rejection, replay of partial failure, rollback when saving the receipt fails, separate endpoint key namespaces, and independent unkeyed requests. Existing API, concurrency, atomic persistence, migration, and docs regressions remain covered.

```bash
cargo +1.83.0 test --locked -p videnoa-controller --lib --test task_api --test task_api_concurrency --test persistence_migrations --test persistence_atomic --test controller_docs
cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.83.0 fmt --all -- --check
bash scripts/tests/controller_docs_test.sh
```

The targeted suite passed 74 tests. Migration checks cover fresh databases and upgrading an existing database while preserving earlier constraints.
