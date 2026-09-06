# Controller media symlink support

## Decision

The user explicitly requested removal of the blanket media symlink rejection
following a NAS batch failure caused by a download-directory alias. Media input
and output paths, batch scanning, configured media roots, and path suggestions
now accept symlinks. No configuration switch is required.

## Path semantics

- Resolve the existing prefix to a real absolute target before opening media.
  Preserve missing output descendants; tasks persist resolved input/output paths.
- Resolve output parents only: an existing final output link is an occupied
  destination, not a write target or proof of a completed publication. Recovery
  rejects such a link even when its target has the expected output bytes.
- Check both the requested spelling and the resolved target against Controller
  private data/temp boundaries. An alias does not grant access to private storage.
- Parse the caller's batch glob before resolving the fixed directory prefix.
  Literal brackets/braces introduced by a symlink target must not become patterns.
- Batch traversal follows resolved media links, deduplicates canonical file paths,
  and tracks visited (real directory, pattern index) pairs to stop directory cycles.
  Existing entry/depth/file limits remain. Broken/unavailable matched aliases are
  skipped; an unavailable explicit directory/input still fails.
- Completion can offer media links but hides private aliases. No frontend change.
- Admission pins the link target, not the alias. Retargeting an alias afterward
  does not redirect a stored task. Existing legacy alias paths resolve again when
  accessed and remain subject to durable identity/content checks.

## Retained guarantees

Actual descriptor opens still use no-follow operations on resolved paths, so
replacement with a new symlink during access fails rather than redirecting I/O.
Root/parent identity checks, regular-file requirements, full SHA-256 content
identity, metadata validation, and output no-clobber behavior remain. Private
SQLite/config/temp artifacts retain their existing stricter handling; this is a
media-path policy change. No migrations, dependencies, authentication, or Worker
protocol changes are included.

One full hash per intake/upload admission remains. Filesystem canonicalization
adds metadata operations, not content reads. Hard-link aliases are not deduplicated
by inode; batch deduplication uses resolved pathnames. Retained file descriptors
are still not immutable snapshots against concurrent in-place writes.

## Verification

Tests use synthetic bytes in temporary directories, not production NAS media.
Coverage includes linked configured roots and media files, a relative directory
alias pointing to a bracket/brace-named directory, batch task persistence with
resolved paths, cycles and duplicate aliases, private aliases, link retargeting,
output creation/no-clobber, unchanged upload/content verification, and publication.

```sh
cargo test --locked -p videnoa-controller --test task12 --test task13 --test task_api --test path_capabilities --test workspace_paths
cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo test --locked -p videnoa-controller --all-targets
bash scripts/tests/controller_docs_test.sh
```

All checks should pass. Rust 1.98.0 is pinned by the repository. Modified source
modules are also checked directly with rustfmt because include-based module
registration prevents cargo-fmt from traversing every source file.

Focused verification passed: path capabilities 9, Task 12 uploads/downloads 25,
Task 13 publication/recovery 42, task/batch API 41, and workspace paths 9 (126
checks total). Strict Clippy, workspace/direct-module formatting, and Controller
documentation checks passed. Tests ran on Linux; Windows execution and the user's
live NAS deployment were not exercised.

Final full Controller suite: 546 passed, 0 failed, 1 ignored across 49
test harnesses; exit status 0. The ignored test is the existing opt-in Argon2
contention stress test.
