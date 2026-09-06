# Videnoa Controller Task 4

## Security boundaries

- Runtime authentication reads only an Argon2id PHC file. Password rotation is detected
  through a SHA-256 hash fingerprint stored with each session, invalidating both old
  cookie sessions and old Bearer credentials without restart.
- Session and CSRF values are independent random 256-bit URL-safe values. SQLite stores
  only SHA-256 digests, expiry timestamps, and the password-hash fingerprint.
- Cookie-authenticated mutations require direct Host/Origin agreement and a matching
  CSRF header. Bearer requests are CSRF-exempt, and no permissive CORS layer is installed.
- Login throttling is direct-IP scoped at five failures per five minutes, with the sixth
  failed request returning 429 and no credential reflection.

## Filesystem boundaries

- Configured relative roots resolve once against the Controller process directory; the
  retained root is stored as an absolute path plus a `cap_std::fs::Dir` descriptor and
  descriptor-derived identity snapshot.
- Every root and descendant directory component is inspected for symlinks and opened
  with `cap_fs_ext::DirExt::open_dir_nofollow`, closing check/open symlink-swap escapes.
- Inputs are reopened through the retained descriptor and rechecked for file identity,
  size, mtime, and current configured-root identity before transfer. Outputs recheck root
  and parent identity and use create-new, no-follow leaf creation so collisions never
  overwrite existing files.

## Verification

- Current strict Clippy, Controller tests, release build, Rust 1.83 tests/release build,
  full workspace tests, live HTTP/CLI probes, secret scans, and dependency-tree checks pass.
- Sanitized evidence is stored under `.omo/evidence/videnoa-controller/task-4/`.
