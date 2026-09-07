# Controller operation diagnostics

## Problem and scope

NAS publication initially failed as `publication_failed`, then retried as
`publication_ambiguous`. Both publication helpers discarded the underlying
error; copy recovery also lost the operation that failed. The lifecycle log
reported the code and directed operators to an equally generic task message.
The user requested substantially more useful diagnostics.

## Correction

- Internal `OperationError` retains the original typed transfer error plus a
  static operation name. Result annotation does not change error classification.
- Every fallible copy/marker operation records context: source/destination opens,
  metadata/identity, hash, prefix read, write/flush, file/directory sync, marker
  creation/write/install/read, and verified-source removal.
- Semantic conflicts have distinct operation names (missing marker, mismatched
  marker length/evidence, missing source, changed content, invalid final, etc.).
- Verification, publication admission, no-replace rename, legacy staging, and
  final-file recovery no longer silently discard errors. Propagated parent-sync
  errors are logged before preserving their original recovery behavior.
- Detailed publication/verification ERROR events carry task/attempt/state,
  operation, safe reason, I/O kind, and raw OS error code when available. The
  lifecycle ERROR remains the confirmation of a committed task failure.
- New task/attempt failure messages preserve the operation and safe cause.
  Old generic messages cannot be recovered without new evidence or retry.
- Download artifact preparation/recovery and local cleanup log nested local I/O
  diagnostics before retry. Remote cleanup logs the existing typed HTTP/network
  cause and worker identity instead of silently scheduling retry.
- No raw request paths, URLs, headers, bodies, or arbitrary error-chain messages
  enter diagnostics. OS messages are reconstructed from numeric errno. Custom
  OS-less errors expose their kind only. Remote client classifications remain
  unchanged; this is not a raw DNS/TLS/HTTP trace facility.
- No hashing, file ownership, copy ordering, retry policy, task states, protocol,
  authentication, configuration, or database schema changes.

## Regression evidence

Tests use synthetic file bytes and filesystem faults, not real media. A directory
at the pending-marker leaf causes a real filesystem failure after creating the
empty final file. The first durable failure identifies `copy.create_pending_marker`
with the OS error; explicit retry identifies `copy.marker_missing`. Both source
and final artifacts are preserved. This reproduces the *shape* of the NAS failure,
not proof of its original underlying cause.

Log-capture tests verify default-visible ERROR/WARN operation and correlation
fields, nested errno preservation, and omission of private sentinel paths/custom
error strings. Existing task12/task13 tests retain content identity, no-clobber,
crash recovery, and retry behavior.

## Validation commands

```bash
cargo +1.98.0 test --locked -p videnoa-controller --lib diagnostics::tests
cargo +1.98.0 test --locked -p videnoa-controller --test task12 --test task13
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.98.0 test --locked -p videnoa-controller --all-targets
bash scripts/tests/controller_docs_test.sh
```

Changed implementation modules also require direct rustfmt checks with
`--edition 2021 --config skip_children=true`, since source topology uses include!.
Expected: all checks exit zero. No frontend changes or dependency upgrades.


Final validation: 553 passed, zero failed, one ignored across 50 full-suite
harnesses. Focused task12/task13: 70 passed; diagnostic unit tests: two passed;
full library tests: 33 passed. Strict Clippy, Cargo/direct-module formatting,
documentation checks, and staged Secret Guard scan passed. The first full run
failed an existing concurrent INFO log-capture assertion; the new capture test
was changed to use the production default EnvFilter rather than a separate WARN
maximum. Both library and complete-suite reruns passed; production filters were
not changed.
