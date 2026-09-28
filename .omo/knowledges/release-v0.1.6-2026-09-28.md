# v0.1.6 release preparation

## Candidate

- Release branch: `release/0.1.6`, based on dev `503fcb5` (docs-only on top of
  `04f2fea`).
- All five workspace packages and their lockfile entries advance from 0.1.5 to
  0.1.6. No external dependency version changes are intended by release prep.
- Publication follows the master-triggered Release Workflow after candidate CI
  passes. The workflow creates the `v0.1.6` tag; do not create it in advance.
  The GitHub release body uses `generate_release_notes`; the notes below are the
  curated summary.
- `misc/bin_win64.zip` was replaced before this release (see
  `windows-4k-workflow-user-report-2026-09-28.md`), so the Windows package picks
  up the GPL FFmpeg without further steps.

## Release notes

Worker video pipeline

- Resize/Rescale now work in video jobs as the trailing processing nodes before
  VideoOutput. FFmpeg applies them in the final encode pass. VideoOutput
  `width`/`height` are optional and really resize the output (#5).
- Lanczos is the new default algorithm for Resize/Rescale.
- The SuperResolution `scale` is checked against the model's native scale.
- Unsupported graphs, and encoders missing from the bundled FFmpeg, are
  rejected before a job is queued (#5, #6).
- Fix FP32 RealESRGAN output being about half as bright. The model was fed
  0–255 input instead of 0–1.
- Fix `videnoa run` aborting at exit with CUDA after a successful workflow.
- Decoder failures and partial frames now fail the job. Cancellation now stops
  scalar and nested workflows between steps. Declared WorkflowInput types are
  preserved.
- Previews run the real SR/Resize/Rescale processors, in managed, bounded
  sessions.

Windows

- The bundled FFmpeg is now the BtbN n8.1.3 GPL build with libx264 and libx265
  (#6).
- The path autocomplete stays navigable. The server returns plain `G:\...`
  paths instead of `\\?\` paths, and the UI understands `\` (#4).
- File API paths are correct when the workspace is on another drive.

Controller

- The event stream recovers after the machine sleeps.
- Paths are reopened after a root replacement.
- Publication falls back to copying when a Linux no-replace rename returns
  EINVAL.
- The task view reads the global counts once per view.

CI and release

- Windows CI now fails on the first failing Rust test. Previously pwsh hid
  every failure except the last command's.
- Package smoke jobs and release packaging jobs verify the bundled FFmpeg's
  encoders, filters, real x264/x265 encodes and mkvpropedit statistics tagging.
- The Windows release archive step now fails when 7z fails.
- Linux release asset downloads are retried.

## Upgrade notes

- VideoOutput no longer has an `fps` port; the frame rate is always preserved.
  A saved `fps` param is ignored. Only a connection into `fps` now fails
  validation.
- A SuperResolution node whose `scale` differs from a known model's native
  scale now fails. Use the native scale plus Resize/Rescale, or set VideoOutput
  width/height.
- Brightness of FP32 RealESRGAN output changes, because it is now correct.
  Earlier outputs from these models were too dark.

## Candidate verification

```bash
cargo metadata --locked --offline --format-version 1 --no-deps
cargo fmt --all -- --check
node scripts/tests/validate_ci_release_workflows.test.mjs
node scripts/validate_ci_release_workflows.mjs
bash scripts/tests/controller_docs_test.sh
bash scripts/tests/controller_archive_root_files_test.sh
bash scripts/tests/package_controller_test.sh
bash scripts/tests/docker_slimming_contract_test.sh
git diff --check
```

Result: every command passed locally on 2026-09-28, and the five workspace
packages resolve to 0.1.6. The candidate CI result and the published artifacts
are recorded after they complete.
