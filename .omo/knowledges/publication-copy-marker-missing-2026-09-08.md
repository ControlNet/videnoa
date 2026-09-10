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

## Follow-up: final output is zero bytes

The user confirmed that the existing destination is zero bytes. Together with marker_missing, this is consistent with interruption/failure after exclusive destination creation and before marker installation or byte copying. It does not independently prove the cause: an external empty file or subsequent truncation remains possible. Prioritize the original operation error (destination file/parent sync, pending-marker creation/write/sync/install) and verified-source availability. Do not treat manual retry as marker repair. No destination or temporary artifact was modified.

## Original failure supplied

Task `4d189b90-a7df-47e3-b2b9-3aa6f51e2a46`, attempt `fa7a2cc0-9a22-4d96-8723-e0971494db18`, entered Publishing at 2026-09-07T19:14:07.912401Z. At 19:14:16.945988Z it failed with `operation=copy.sync_destination_parent`, `io_kind=None`, `raw_os_error=None`, reason `output_parent_changed`; lifecycle persisted publication_failed, retryable=true. The user confirmed a zero-byte destination and subsequent marker_missing on retry.

`RootedOutput::sync_parent` first calls `open_parent(false)`. That reopens the anchored parent and compares its device/inode, plus identities for any created intermediate directories. OutputParentChanged is an application identity rejection before `sync_directory`, not an OS fsync failure. `root::identity` compares dev and ino only; ordinary mtime/size changes are not tested. This source matches v0.1.4.

The observed sequence is consistent with exclusive empty final creation and file sync succeeding, then parent revalidation failing before marker installation and before byte copying. This explains both the zero-byte remnant and follow-up ambiguity. Directory identity replacement/reported-identity instability remains unexplained without the NAS filesystem/mount details. Requested NAS OS, Controller output path, filesystem and container/network/union mount topology. Do not infer a particular NAS filesystem or blame media metadata updates from this log.

The failed-publication recovery design demonstrably leaves an unrecoverable-by-retry-alone empty-file/evidence state along this path. Resolving parent identity compatibility and safely handling owned empty remnants are separate concerns. No files or production code were changed.

## Recurrence on 2026-09-10

Task 2559addd-0720-41cf-a4f1-35c85d669ab0, attempt 51bba13d-2e94-4ecd-ba21-696e0ff38bbe, entered Publishing at 11:51:37 UTC and failed at 11:51:42 with copy.sync_destination_parent: output_parent_changed, io_kind=None, raw_os_error=None. This again indicates application directory device/inode revalidation rejection before directory synchronization, not an OS fsync timeout. Current copy code uses the same operation label both before installing ownership evidence and after copying and hashing the destination; the log alone cannot distinguish the two sites or establish destination size.

The lifecycle reports a retryable publication failure for this task, not a process-fatal error. The subsequent health warning is for worker 6195ab65-8bd4-44a2-96e6-470e79fcfa5e and supplies no causal evidence for this filesystem failure. The empty-output cleanup fix is present in current source but does not resolve directory identity drift. The deployed build, exact output path, and host/container mount mapping remain to be confirmed. Asked whether the output is still an Unraid /mnt/user share. Do not assume actual directory replacement versus unstable filesystem identity without mount and filesystem evidence. No production state or source implementation changed for this diagnostic follow-up.
