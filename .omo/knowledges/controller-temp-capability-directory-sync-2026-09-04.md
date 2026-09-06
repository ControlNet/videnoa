# Controller Temp Capability Directory Sync

## Confirmed failure

`cap_std::fs::Dir` returned by `open_dir_nofollow` may retain an `O_PATH`
descriptor on Linux. Converting that descriptor to a standard file and calling
`sync_all` fails with `EBADF`, even though descriptor-relative child operations
continue to work.

## Correct pattern

- Retain the capability directory for identity and descriptor-relative access.
- For durable directory synchronization on Unix, call `openat` on `"."` relative
  to the retained directory with `RDONLY | DIRECTORY | CLOEXEC`, then `fsync` the
  returned handle.
- Do not reopen the directory through its ambient descendant path.

## Security boundary

- Classify artifact leaves with descriptor-relative `symlink_metadata` before
  removal. Reject symlinks and non-regular nodes instead of silently consuming
  hostile substitutions.
- Keep the final `remove_file` descriptor-relative; it removes the named leaf and
  does not follow a replacement symlink.
- Convert temp workspace and artifact recovery failures into the scheduler's
  durable download retry outcome at the download boundary.

## Fixture contract

Every configured root passed to `PathCapabilities::open`, including `temp_root`,
must exist before capabilities are retained.
