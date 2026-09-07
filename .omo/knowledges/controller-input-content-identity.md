# Controller Input Content Identity

Controller upload admission must not treat filesystem metadata as proof that input bytes are unchanged. A fast same-size remove-and-recreate can reuse device, inode, length, and nanosecond modification time.

Persist `InputSnapshot::content_identity()` through the separate nullable `InputContentIdentity` column and compare it before any remote PUT. The identity is the first 16 bytes of SHA-256. Existing `InputIdentity` rows keep their device/inode meaning, and migrated rows with no content identity retain metadata-only admission. Rooted reopen also retains device/inode, length, and modification-time checks.

When hashing a capability-opened file:

- Read from the descriptor rather than reopening an ambient path.
- Rewind the descriptor before returning it for upload.
- Re-read descriptor metadata after hashing and fail closed if it changed.
- Keep Cargo verification serial because Controller integration suites use process and network fixtures.

## Superseding correction (2026-09-07)

See [Controller input scans and transfer inactivity correction](controller-input-scans-transfer-inactivity-2026-09-07.md).
The historical behavior above is retained as a record. Duplicate intake/upload
hashes are removed, and upload uses an inactivity watchdog. Any advice above to
set transfer timeout beyond the complete upload duration is superseded; the
900-second default now bounds inactivity, not total transfer duration.
