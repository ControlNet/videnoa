# Copy publication with retained destination directory

## Motivation and boundary

The user authorized replacing repeated parent inode checks during publication with retained directory handles and final visible-file verification after recurrent copy.sync_destination_parent: output_parent_changed reports on Unraid. The exact mount-level source of inode drift has not been measured, and local tests do not establish production Unraid compatibility.

## Implementation

CopyDestination already owns the exclusively opened file and its original directory for empty-file rollback. Both copy publication destination-directory synchronization steps now use that retained directory rather than reopening RootedOutput's parent and comparing saved directory identities. File creation and copying still operate on the retained handles.

RootedOutput::open_copy_final resolves the visible destination with root identity validation, no-follow intermediate traversal, and no-follow regular-file opening, without comparing intermediate directory inode snapshots. Initial publication admission and destination acquisition retain existing checks. Root replacement remains rejected; file identity stability is still required.

Before ownership-marker installation and copying, validate_visible compares the visible file device/inode with the retained file. A real replacement fails before bytes are copied and preserves prior empty-file rollback behavior. After copying, the visible final file must match the recorded file identity, expected size and SHA-256. The retained destination directory is synchronized, and the visible file identity is checked again before removing the verified source. No-overwrite, ownership-marker/prefix verification, symlink rejection, and recovery behavior remain in force. The rename fast path and recovered-source-missing path are unchanged; this patch addresses the reported copy path.

## Regression scenarios

All fixtures are synthetic files and directories, not production media. A unit test changes only the saved parent inode and proves retained sync and visible-file validation succeed while the strict open_final check would reject it. Another test replaces the parent with a symlink: sync still targets the retained directory, visible validation rejects the link, and empty rollback removes only the original file. Existing pre-copy parent replacement tests preserve the replacement empty file and recover the same attempt. A new post-marker directory replacement with an identically sized/content-matched foreign file proves file identity mismatch still rejects publication and retains both verified source and original copied output.

The first full run found a test expectation missing the diagnostic suffix ': evidence_conflict'; production correctly rejected the replacement. Corrected the assertion and restarted the full suite. Clippy's function-length limit also prompted binding the already-open file once for metadata reads, with no behavioral change.

## Verification

```bash
TMPDIR=/run/user/1008 cargo test -p videnoa-controller --no-fail-fast -- --test-threads=4
cargo clippy -p videnoa-controller --all-targets -- -D warnings
```

The temporary storage location is specific to this development host; it keeps cross-filesystem tests distinct from /dev/shm while avoiding variable VM disk I/O. Production filesystem confirmation still requires a build containing this patch and a publication on the user's mount. No production container, task, or media file was modified.

Final results: the complete current-code controller suite exited 0 with 586 passed, 0 failed, and 1 pre-existing ignored stress test. Strict all-target Clippy, rustfmt checks for changed Rust files, staged whitespace checks, and secret scanning passed.
