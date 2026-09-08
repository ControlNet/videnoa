# Diagnosing copy.marker_missing in v0.1.4

Reported task details: publication / publication_ambiguous / Manual Retry Available / `copy.marker_missing: evidence_conflict`. No task ID, deployed build identity, original failure, paths, or file metadata were supplied.

## Exact meaning

The v0.1.4 `scheduler/publication_copy.rs` copy/recovery branch opened the final output as a regular file and `read_marker` returned None for the verified source sibling `publication-copy.evidence`. This identifies missing ownership evidence, not a hash mismatch, relay error, or proof of final-file corruption. Malformed markers, permissions errors reading markers, and mismatched content use other operation names.

The marker belongs in the task's Controller temporary workspace next to `output.<extension>.verified`, not the media directory. It stores destination file identity, expected size, and expected SHA-256. Recovery with a valid verified source and an existing final requires the marker even if both files happen to contain identical bytes.

Copy publication creates the final destination exclusively, syncs the empty file and parent directory, and only then writes/syncs/installs the marker via `publication-copy.pending`. A process interruption or filesystem failure after final creation but before marker installation leaves a destination with no evidence. A subsequent retry reports marker_missing. A same-name file created externally or deleted evidence can produce the same state. The code does not establish which happened on this NAS.

The existing synthetic regression `publication_copy::marker_creation_failure_preserves_operation_and_followup_ambiguity_reason` creates a filesystem fault at the pending marker, observes initial publication_failed, retains a zero-byte final and intact source, then observes this exact publication_ambiguous message on retry. Reviewed its source; it was not rerun for this diagnosis.

## Recovery implications

Manual retry rechecks the same evidence and does not automatically repair a missing marker. If the observed state is unchanged, another retry should fail the same check. Publication retry does not repeat AI processing. The v0.1.4 per-task duplicate-finalizer permit prevents concurrent finalizers but does not close the destination-create/marker-persist interruption window or repair older remnants.

Before proposing a filesystem change, collect the task ID, first failure preceding this message, final output type/size, and listing of the task workspace (verified source, evidence, pending). Preserve both final and verified source; do not delete artifacts or fabricate evidence to force a retry. In particular, a zero-byte output plus an earlier copy.sync_destination_parent or copy.create_pending_marker error helps distinguish the pre-marker failure path. No production state or product source was changed.
