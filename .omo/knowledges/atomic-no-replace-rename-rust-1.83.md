# Atomic no-replace rename on Rust 1.83

- rustix 1.1.4 is current as of 2026-09-03, MSRV 1.65, and exposes safe `rustix::fs::renameat_with(..., RenameFlags::NOREPLACE)` behind `fs`; Linux implementation is renameat2.
- winsafe current 0.0.29 requires Rust 1.87. winsafe 0.0.23/0.0.24 expose safe `MoveFileEx`, but require Rust 1.84. winsafe 0.0.22 is Rust 2021-compatible but only exposes safe `MoveFile` (not MoveFileEx).
- renamore 0.3.2 exposes safe consumer API `rename_exclusive`; Linux uses renameat2 + RENAME_NOREPLACE and Windows uses MoveFileExW with flags 0. It has no rust-version field and uses Rust 2021; verify with Rust 1.83 CI because release is old.
- Windows `MoveFileExW` replacement is opt-in via MOVEFILE_REPLACE_EXISTING; flags 0 is the no-replace form.
