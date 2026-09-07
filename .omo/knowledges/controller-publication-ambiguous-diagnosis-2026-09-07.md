# Diagnosing NAS publication ambiguity

Source inspection at dev `82bf1af8a27654034e08bfd0dcfc7b0201d13cf1`.
The reported retry entered Publishing, then failed with `publication_ambiguous`
about 12 seconds later. The NAS deployment revision and filesystem evidence are
not yet known; no specific root cause has been established.

- HTTP 200 on retry acknowledges scheduling; it does not confirm publication.
- `scheduler/publication_failure.rs::fail_ambiguous` deliberately shares one
  message across multiple branches. Neither the error code nor elapsed time
  identifies the failing check. Task details may repeat that generic message.
- Reconciliation rejects missing durable expected size/hash, invalid output path
  capabilities, present legacy staging artifacts, nonregular final outputs, and
  missing/invalid verified sources unless a completed final can be recovered.
- With a valid verified source and an existing regular final, copy recovery
  requires `publication-copy.evidence`. It binds expected size/SHA-256 and the
  destination device/inode. A missing, malformed, mismatched marker, changed
  destination identity, excessive destination length, or differing copied prefix
  prevents recovery. Matching final bytes alone do not prove ownership while the
  verified source remains present.
- EXDEV has a copy fallback; different media/temp mounts alone do not explain this
  error. Historical same-filesystem-only advice is superseded by
  `controller-progress-cross-mount-move-2026-09-06.md`.
- Copy fallback has a documented crash gap between exclusive final creation and
  durable marker persistence. NAS file replacement or movement could also affect
  identity, but neither has been demonstrated in this incident.

Collect deployed image/revision, original failure before retry, task output path,
final file type/size, and the task's temp workspace evidence before choosing a
repair. The workspace is `<temp_root>/<task_id>/`; relevant files include
`output.<extension>.verified` and `publication-copy.evidence`. Preserve these
files and any final output. Do not delete artifacts or reset database state just
to bypass ambiguity. No production behavior was changed during this diagnosis.

## Publication timing follow-up

The copy loop in `scheduler/publication_copy.rs` uses 64 KiB buffers and has no
total-duration or inactivity deadline. `scheduler/recovery_dispatch.rs` awaits
publication directly; the orchestration scan timer does not cancel active stages.
HTTP `transfer_seconds` does not bound local publication copying or hashing.
Shutdown cancellation aborts orchestration stage tasks in `recovery_scan.rs`;
process termination and filesystem errors can still interrupt publication.
An underlying network filesystem can have its own timeout/error behavior.
The reported ambiguity therefore does not establish a Controller copy timeout.


## Superseding correction: explicit publication retry

Publication ambiguity now permits manual retry on the existing attempt. Remote
state ambiguity remains blocked. The classifier requires the publication stage;
retry does not create a compute attempt or change task paths. Failed tasks are
not automatically scheduled, and each explicit retry repeats the unchanged
ownership, content, path, and no-clobber checks. An unresolved conflict fails
again, remaining manually retryable.

Migration 0012 enables retryability for existing failed publication-ambiguity
rows and their matching current failed attempts, incrementing changed versions.
It does not start publication or modify files, expected hashes, or other failure
classes. The frontend retry policy and guidance now match the backend.

The user subsequently supplied the initial `publication_failed` followed by
`publication_ambiguous` on manual retry. This confirms distinct failure classes,
not the precise underlying filesystem error. Local publication still has no
transfer deadline; exposing underlying I/O diagnostics is separate work.

Regression fixtures use synthetic output bytes. Coverage checks repeated manual
retry preserves an unowned empty final and verified source, then publishes after
the test operator preserves the conflict elsewhere, without new AI work. Legacy
migration coverage checks both rows, unchanged Failed state, idempotence, and
successful same-attempt publication after explicit retry.


Validation commands for this correction (Rust is currently pinned to 1.98.0):

```bash
cargo +1.98.0 test --locked -p videnoa-controller --test lifecycle --test task13 --test persistence_migrations
cargo +1.98.0 fmt --all -- --check
rustfmt +1.98.0 --edition 2021 --config skip_children=true --check crates/controller/src/lifecycle/classification.rs crates/controller/src/lifecycle/failure.rs crates/controller/tests/lifecycle/retry.rs crates/controller/tests/task13/publication_ambiguity.rs
cargo +1.98.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.98.0 test --locked -p videnoa-controller --all-targets
npm --prefix controller-web test -- src/tasks/taskActionPolicy.test.ts
npm --prefix controller-web run typecheck
npm --prefix controller-web run lint
npm --prefix controller-web run build
bash scripts/tests/controller_docs_test.sh
```

Expected: all checks exit zero. Focused Rust coverage: 68 passed; frontend policy:
19 passed. The new typed-failure regression was confirmed failing before the fix.

Final validation: full Controller all-targets suite passed (550 passed, zero
failed, one ignored across 50 harnesses). Strict Clippy, both formatting checks,
frontend typecheck/lint/build, and documentation checks passed.
