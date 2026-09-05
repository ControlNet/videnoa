# Controller Direct Publication

- Verified downloads and their size/SHA-256 evidence remain under `temp_root` until publication.
- Output roots must contain no Controller-created intermediate files. Publication is a direct atomic no-replace rename from the verified temp leaf to the exact final leaf.
- `temp_root` must be disjoint from every output root and share each output root's filesystem. Root device mismatch fails capability opening; a nested-mount `EXDEV`/not-same-device failure is mapped to `PathError::CrossFilesystemPublication`. There is no copy fallback.
- New `Publishing` rows persist `destination_staging_name = NULL`. A non-null value is legacy evidence: a missing sibling proceeds through direct temp/final recovery, while an existing, non-regular, invalid, or unreadable legacy sibling is conservatively terminalized as `publication_ambiguous` without deleting it.
- Recovery matrix: temp present/final absent retries rename; temp absent/final exact completes cleanup; both present, final mismatch, non-regular final, or missing both are ambiguity. Publishing recovery hashes the existing temp leaf directly against durable DB size/SHA-256 without reading, reconstructing, or deleting the download sidecar.
- Publication re-hashes the verified source immediately before rename so a replaced temp leaf cannot reach the output root.
- The post-rename checkpoint intentionally occurs before directory sync/lifecycle CAS so restart tests prove exact-final recovery without another remote compute request. Durability syncs the destination parent before the source parent; a sync error propagates without a lifecycle transition, leaving the task in `Publishing` for reconciliation. Matching-final recovery repeats the destination-parent sync and the retained temp-parent sync when available before `FinishPublication`, so a crash or sync failure never turns an unsynchronized rename into accepted publication.
- Linux uses `renameat2(RENAME_NOREPLACE)` through retained directory capabilities. Windows uses `renamore::rename_exclusive`; cross-volume behavior is typed, but the Rust 1.83 Windows std target was unavailable in the Linux verification environment.

## Filesystem Regression Count

`task21_filesystem` now contains 17 tests instead of 22 because six destination-staging-era cases were removed from its included `task13/publication.rs` module and one clean-output-root direct-publication case was added, for a net reduction of five. The removed behaviors remain covered under the direct model:

- Staging replacement maps to `verified_source_replacement_before_rename_never_reaches_output_root`.
- Corrupt staging and matching final plus staging map to the non-destructive both-present tests in `task13/publication_ambiguity.rs`.
- Valid hidden-staging resume is intentionally obsolete because Controller intermediates under output roots are forbidden; `missing_legacy_staging_leaf_recovers_from_direct_temp_evidence` covers compatible legacy rows.
- Non-regular and FIFO staging map to `legacy_staging_fifo_is_ambiguous_without_blocking_or_mutation` and `legacy_staging_symlink_is_ambiguous_without_touching_its_target` in `task13/publication_nonregular.rs`.
- Final-node and source-node safety is retained by the final FIFO/symlink/directory and verified-temp FIFO tests in `task13/publication_nonregular.rs`.
