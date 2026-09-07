# Legacy Linux Package Archive

- `scripts/package_dist.sh` produces the existing `videnoa/` directory; it does not create the release archive.
- Linux smoke and release archive through `scripts/package_dist_archive.sh` with 2000 MiB volumes. Names remain `videnoa-linux64-smoke.7z` and `videnoa-linux64-<version>.7z`; split output starts at `.7z.001`.
- The helper requires free output capacity for the full bundle plus 64 MiB before starting p7zip. This intentionally overestimates the compressed result and prevents p7zip 16.02 from failing mid-write with opaque `E_FAIL` on variable hosted-runner disk headroom.
- Verification accepts `.7z.001` first and falls back to an unsplit `.7z`, runs `7z t` for integrity, then checks the exact `videnoa/` root from `7z l`.
- Focused regression: `bash scripts/tests/package_dist_archive_test.sh`. Workflow preservation: `node scripts/tests/validate_ci_release_workflows.test.mjs`.
