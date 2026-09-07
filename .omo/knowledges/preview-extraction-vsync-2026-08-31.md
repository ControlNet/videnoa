# Preview extraction FFmpeg sync mode

Date: 2026-08-31

## Root cause

`POST /api/preview/extract` passed `-vsync vfn` to FFmpeg. FFmpeg 4.4.2 rejects that value with `Expected number for vsync but found: vfn`, so valid videos returned HTTP 500 before decoding.

Changing only the sync mode from `vfn` to `vfr` made the same extraction command produce all 10 requested PNG frames. The fixture probe returned 120 frames, so the count-derived select interval of 12 was valid and unrelated to the failure.

## Fix and evidence

- `crates/core/src/server/mod.rs` defines `PREVIEW_VSYNC_MODE` as `vfr` and uses it for the extraction command.
- The focused regression test verifies the configured mode.
- `cargo test -p videnoa-core` passed with 564 tests passed and 10 ignored.
- `npm ci && npm run build` and `cargo build --release -p videnoa-app` passed.
- Live HTTP QA returned 201 with 10 frame URLs; the first URL returned a 1920x1080 RGB PNG.
- Browser QA through Editor > Preview rendered all 10 frames at 1920x1080 with no console errors.
- A nonexistent source path still returned HTTP 400 with `video file not found`.

## Known unrelated validation issue

`cargo clippy -p videnoa-core --all-targets -- -D warnings` remains blocked by pre-existing warnings in unrelated files, including redundant field names in `frame_interpolation.rs` and `super_res.rs`.
