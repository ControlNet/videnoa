# Empty publication output cleanup

## Report and scope

On Unraid, task `4d189b90-a7df-47e3-b2b9-3aa6f51e2a46` failed at `copy.sync_destination_parent: output_parent_changed` after exclusive destination creation. The final file was zero bytes; no copy ownership record existed, so retry failed with `copy.marker_missing`. The user explicitly requests cleanup of the zero-byte file on publication failure to make retry possible. The accidentally pasted provider-logo request was withdrawn and is unrelated.

Worktree: `/tmp/videnoa-publication-empty-output`, branch `fix/publication-empty-output`, based on released master `7efc672`. This preserves the v0.1.4 publication exclusion fix. The existing dev worktree and its untracked UI design files were not modified.

## Implementation

- `CopyDestination` retains the original directory descriptor and open destination file for new copy outputs. It arms empty-file cleanup immediately after exclusive creation and disarms after successful byte copying.
- Error returns and async-task cancellation drop the guard. Cleanup checks both the open file and the no-follow directory entry: both must still be regular, zero-length, and share the same device/inode. It removes through the retained directory handle and synchronizes that directory, without reopening a potentially replaced parent path.
- Already present files, replacement entries, symlinks, and nonempty partial output are preserved. Previously opened owned outputs are not armed for new-file cleanup.
- Original stage diagnostics remain authoritative; cleanup failure adds `copy.cleanup_empty_destination` with safe I/O kind and OS code. Verified source and compute identity remain available for retry.
- A new pre-marker `PublicationCopyCreated` checkpoint makes the failure/cancellation window deterministic in tests. No timeout, path-identity check, marker validation, or no-overwrite rule was relaxed.
- Filesystem unavailability, failure to verify ownership, and hard process termination can prevent cleanup. This is not automatic deletion of historical unowned zero-byte remnants, and it does not claim to resolve Unraid directory-identity instability. No NAS files or running deployment were modified.

## Regression evidence

The updated marker-write-failure regression failed before the fix because the destination still existed. It passed afterward and completed the same attempt on retry without another Worker Run request.

Synthetic tests cover directory replacement producing the exact reported output_parent_changed error, cleanup through the original directory, preservation of a different empty file under the new directory, retained verified bytes, marker failure, cancellation before/after marker creation, and successful retry. Path unit tests cover nonempty output, replacement zero-byte files, preexisting owned output, and replacement symlinks. Fixtures are test-only bytes/directories under disposable temporary roots; production media is never used.

## Verification commands

```bash
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-controller --test task13 --lib
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo test --locked -p videnoa-controller --test task20 --test task21_filesystem --test task21_concurrency --test task21_resources -- --test-threads=4
CARGO_TARGET_DIR=/tmp/videnoa-iroh-target cargo clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
rustfmt --check --edition 2021 crates/controller/src/paths/publication_copy.rs crates/controller/src/paths/publication_tests.rs crates/controller/src/scheduler/publication_copy.rs crates/controller/src/scheduler/transfer_checkpoint.rs crates/controller/tests/task13/publication_copy.rs crates/controller/tests/task20/fault_matrix_local.rs
bash scripts/tests/controller_docs_test.sh
git diff --check
```

Expected: no failures or warnings. Library and task13 checks passed 38 and 46 tests respectively. Clippy, formatting, and documentation contracts passed. Broader recovery, concurrency, filesystem, and resource suites passed 109 tests across four binaries (counts: 33, 57, 18, 1); no failures. Remote CI must verify the pushed candidate independently.
