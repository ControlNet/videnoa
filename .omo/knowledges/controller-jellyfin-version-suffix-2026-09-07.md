# Jellyfin version suffix investigation

## Verified upstream behavior

- Jellyfin episode multi-version support was merged on 2026-05-15 in https://github.com/jellyfin/jellyfin/pull/16828, superseding closed PRs #8004 and #16239. Do not infer current support from the old PRs or movie-only documentation.
- As checked on 2026-09-07, the latest listed preview is 12.0 RC7, released August 31. Jellyfin dropped the `10.` prefix: https://github.com/jellyfin/jellyfin/releases/tag/v12.0-rc7.
- Inspected the actual `v12.0-rc7` sources at `Emby.Naming/Video/VideoListResolver.cs` and `tests/Jellyfin.Naming.Tests/Video/MultiVersionTests.cs`. The TV library path groups parsed files by season and episode (or air date), without requiring the movie folder-name prefix or a particular version separator. Tests cover named suffixes and multiple episodes in one season folder.
- Consequently, in the same show's season directory in a TV library, `Re Zero S03E01.mkv` and `Re Zero S03E01 - AI.mkv` should group under the RC7 implementation. This is a source-based conclusion, not a live media-library test. The old `.AI.mkv` form should also group if parsed to the same episode; replacing dots alone is not what enables TV grouping on older servers.
- Related version-name and playback fixes: https://github.com/jellyfin/jellyfin/pull/17044 (merged July 5).

## Proposed controller change (not implemented)

- Add `jellyfin_version_suffix` to the backend `NamingMode` in `crates/controller/src/tasks/batch.rs` and the frontend `BatchOptions` union in `controller-web/src/tasks/batchTask.ts`.
- Add `Jellyfin version suffix` to `BatchTaskDialog.tsx`, showing a labeled `Version Label` input with default `AI` and the example `stem - AI.ext`. Preserve existing styling and preview flow.
- The minimal API-compatible option is to reuse `middle_extension` as the label, documented per naming mode; do not rename existing serialized fields because durable batch idempotency fingerprints include their serialization.
- Extend label validation and generate `stem - label.ext` in backend `output_path()`, retaining the original stem and final extension. Both preview and HTTP batch creation share this path. No worker or database migration is needed.
- Changing output location currently unconditionally resets naming to `insert_extension`. Preserve the selected suffix mode; only reset an incompatible `original` selection when moving beside the input.
- Retain existing collision and output-exists checks. Define append-only behavior explicitly for inputs already carrying a version label; do not silently strip episode titles or rename source media.
- Implementation verification should cover custom labels, multi-dot/Unicode stems, original extension case, invalid labels, both output modes, exact preview-to-create paths, collisions, and historical idempotency serialization. Run controller task API tests plus frontend tests, lint, build, and the batch dialog browser suite when implementing.

Only investigation notes were changed during this session; no application implementation or runtime testing was performed.
