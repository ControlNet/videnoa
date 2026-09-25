# Linux rename EINVAL copy fallback

## Deployment evidence

The Unraid Controller container `videnoa` mounts `/mnt/user/appdata/videnoa-controller`
at `/workspace/data`, `/mnt/user/media` at `/media`, and `/mnt/user/downloads` at
`/downloads`. Its active `data_root` is `/workspace/data` (reported as XFS) and
`cache_root` is `/media/.videnoa` (reported as `fuseblk`). A task whose final
output is also under `/media` failed during Publishing at `rename.noreplace`
with Linux `EINVAL` (OS error 22). The same mount means the existing `EXDEV`
copy fallback is not selected. The exact filesystem reason for EINVAL has not
been proven independently, but unsupported `RENAME_NOREPLACE` is consistent
with Linux FUSE behavior.

## Change

Controller still attempts the atomic no-replace rename first. On Linux, only
an actual rename I/O error with raw errno `EINVAL` enters the existing guarded
copy publication path. `EXDEV` continues to use that path. Existing-output,
permission, path-validation, and OS-less `InvalidInput` errors do not acquire
this fallback. The EINVAL event is logged as a path-free WARN before copying.
No filesystem-type pre-detection is used. The copy path retains exclusive final
creation, ownership marker, size/hash verification, and verified-source
preservation on failure. As with the existing EXDEV path, the final filename is
visible while copying; media scanners can observe incomplete bytes.

## Verification and deployment note

The focused Linux errno-classification test passed; `task13` publication and
copy integration tests passed in the broader Controller run. Formatting,
Controller documentation checks, and strict Clippy passed. The full Controller
suite encountered `Database(PoolTimedOut)` in unrelated `task20` tests, including
when run serially; one affected submission test passed when run alone. This
work was not tested on the user's Unraid mount and is not a release/deployment.
The failed task's verified artifact remains on the old cache path; preserving it
allows a later manual publication retry after the fixed Controller is deployed.
