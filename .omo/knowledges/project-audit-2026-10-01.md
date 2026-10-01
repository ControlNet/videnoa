# Project audit — 2026-10-01 (dev `a4ad098`, after v0.1.7)

Four parallel read-only reviews (frame pool and pinned input; colour pipeline;
worker app, server and hygiene; controller and its web) plus dependency
advisories. Every item below was re-read in the code before being recorded.
Nothing here was fixed yet; see the follow-up commits for what was done.

## Verified clean

- Frame pool and pinned RIFE input: no stale-data writers, no leaks, no
  length/capacity misuse, `release_frame` covered for every production sink,
  no new `unsafe`. The pool's 32-buffer cap bounds count, not bytes.
- Colour conversion maths and tags, measured with FFmpeg 8.1 and 4.4 on
  x264/x265/NVENC at 8 and 10 bit, with and without trailing Resize/Rescale:
  BT.709 limited throughout; `media_tools.ps1 -Verify` chain is literally the
  Rust chain. Range follows frame metadata (`pc`/yuvj) automatically; RGB
  sources are unaffected by `in_color_matrix`.
- Worker server: no HTTP-reachable panics, no path traversal in `/api/files`
  or `/api/fs/list`, job-state transitions race-free; env/cwd/temp-path test
  flakiness absent.
- Controller: every `/api` route except health/setup/login is behind auth;
  CSRF + strict Origin/Host; Argon2id with random salt; digest-only tokens;
  constant-time compares; static asset traversal blocked; publication rename
  fallback (EXDEV and Linux EINVAL) and data-root migration are correct.
- No secrets in tracked files. No TODO/FIXME in non-test code.
- `origin/feat/optional-password` is fully merged via PR #1 (136 commits
  behind dev, no open PR): safe to delete.

## Findings, ranked

### Bugs

1. **Required inputs are not validated for frame-chain nodes**
   (`crates/core/src/graph.rs:218-221`, `if has_vf_edge { continue; }`).
   A `VideoOutput` without `output_path`, a `Resize` without size, an SR
   node without `model_path` all return 201 and fail in `compile_graph`.
   Source/sink type and linearity are also compile-time only
   (`compile.rs:153-176, 384-413`); the preview path already calls
   `validate_linear_topology`, the job path should too.
2. **NVENC probe encodes 64x64** (`video_output.rs:119`). NVENC minimums
   (A40 rejects < 144x144) make every NVENC job fail on such GPUs. Fix: the
   lavfi size, e.g. `256x256`; optionally a timeout and per-codec memoisation.
   Parked earlier by request.
3. **Sink execute error swallowed** (`compile.rs:300-303`, `Err(_) =>`
   fallback). "source file does not exist" becomes a later, vaguer error.
4. **Controller: CSRF rotates on every `GET /api/auth/session`**
   (`crates/controller/src/auth/http.rs:160`, one digest per session) and the
   SPA only learns the new proof from that response
   (`controller-web/src/api/client.ts:88-89`). A second tab invalidates the
   first tab's mutations (403) until reload; the client handles 401 but not
   403.
5. **Controller: `purge_expired_sessions` has no caller**
   (`persistence/session.rs:104`). `auth_sessions` grows per login forever.
6. **Preview `ffprobe -count_frames` shares the 120 s budget** with frame
   extraction (`server/mod.rs:2301-2323`, `preview_cache.rs:23`). Long or 4K
   sources time out before extraction starts; the 1000-frame fallback only
   covers unparseable output.

### Risks

7. **Controller login/Bearer limiter is cosmetic** (`auth/service.rs:165-176`,
   `auth/session.rs:136-158`): Argon2 verify runs before the budget check, so
   429 only relabels failures; garbage Bearer tokens from any client cost a
   full Argon2 through a 2-permit semaphore and queue legitimate traffic.
   Check the budget first and skip the hash.
8. **Dependency advisory**: `rustls 0.23.36` RUSTSEC-2026-0285 (TLS 1.3
   handshake messages accepted across encryption levels; transcript still
   authenticated). Fix: `cargo update -p rustls` → 0.23.45. `web/` has 8
   npm advisories, all with semver-compatible fixes; 6 are build tooling,
   2 ship to the browser (`react-router` turbo-stream, `uuid` v3/v5/v6 with
   caller buffer). `controller-web/` is clean.
9. **Untagged-source test clips are tagged under FFmpeg 8.1**
   (`video_input.rs:1399-1456`): `scale=out_color_matrix=bt709` stamps the
   frame and libx264 writes it to the VUI, so the untagged→BT.709 path from
   `a4ad098` is only exercised with FFmpeg 4.4. Add
   `setparams=colorspace=unknown:...` for `tag == None` and assert
   `info.color_space.is_none()`.
10. **StreamOutput emits 4:4:4 H.264** (`stream_output.rs:87-88`, no
    `format=`): rejected by RTMP ingest. Append `format=yuv420p`.
11. **ffmpeg stderr never reaches the job error** (`video_output.rs:336-349,
    411`; `stream_output.rs:143-156, 207`): failures read "exited with status
    N" or "Broken pipe". Keep a bounded stderr tail and append it.
12. **Preview `process_frame` holds the single GPU permit across untimed
    ffmpeg/ffprobe subprocesses** (`server/mod.rs:2456-2481`,
    `preview.rs:157-213`). A hung child blocks all jobs.
13. **Pinned tensor allocation has no fallback**
    (`frame_interpolation.rs:648`): a `cudaHostAlloc` failure (233 MB per
    lane) fails the job instead of falling back to pageable memory. Also
    `extract_tensor_mut()` at `:727,734` is the panicking variant.
14. **Controller test suite is flaky under load**: 5 s SQLite acquire timeout
    (`persistence/database.rs:12,79`); 68/70 failures in a full parallel run
    were `PoolTimedOut` at fixture open, all green in isolation.
15. **Controller SSE sleep/wake**: `resume()` returns early when the browser
    still reports `OPEN` (`SessionEvents.tsx:124-128`); a half-open stream
    shows connected with stale data. Needs a client watchdog.
16. **BT.2020-tagged SDR sources** (transfer untagged) decode with the right
    matrix but are re-tagged bt709 primaries without conversion; output is
    desaturated with a false tag. Pre-existing. Warn or convert.
17. **CI**: actions on mutable tags; `--locked` only on transport; no clippy
    gate for core/app; no `timeout-minutes`; Docker `latest` pushed in
    parallel with packaging.
18. **Docker image installs apt FFmpeg 4.4** (`Dockerfile:103`) while the
    release archive bundles 8.1; README omits `setup_dev_media_tools.sh` and
    says "FFmpeg 4.4+".
19. **CLI ignores `paths.trt_cache_dir`** (`app/lib.rs:641` uses
    `VideoCompileContext::default()`); the server honours it.
20. **Palette offers unusable nodes**: `SceneDetect` and `ColorSpace` are in
    the descriptor but rejected by `validate_video_processing_chain`;
    `ColorSpace`'s `config` output has no consumer since zscale was removed.

### Docs

21. README-controller.md:107-110 states 24 h / 1 h sessions and 10/5/900 s
    timeouts; code and example TOML say 30 d / 7 d and 10/5/300.
22. docs/controller.md:306 and README-controller.md:74-78 still say only
    EXDEV triggers copy fallback (EINVAL was added).
23. Workers described as credential-free though the DTO takes `password`
    and iroh `transport`/`endpoint_id`; "all DTOs reject unknown fields" is
    false for worker DTOs (`deny_unknown_fields` + `flatten`).
24. No user-facing docs for BT.709 output and untagged decoding, the `fps`
    port removal, the Resize/Rescale trailing rule, `PORT`, Node 24.
25. Stale module docs mention zscale (`video_output.rs:1-6`,
    `color_space.rs:1-4`); `media_tools.ps1:36,38` still requires zscale.

### Nits

- 2.9 MB generated `web/design/videnoa-worker-gui.html` is tracked while the
  controller equivalent is ignored.
- `crates/core/src/nodes/stream_input.rs` is not in `nodes/mod.rs` (never
  compiled).
- `createPreset` has no caller; `POST /api/presets` is in-memory only.
- `create_batch` is non-atomic; job history unbounded; limiter map unbounded;
  `GET /api/workers` N+1; SSE one DB read per subscriber per change; host-only
  listener change rejected; no HTTP server timeouts.
- Decoder/preview `scale` lacks `accurate_rnd+full_chroma_int` (±3 vs ±2);
  preview ffmpeg has no `-map 0:v:0`.
- Missing `// SAFETY` at `runtime.rs:312`.
- Unmaintained transitive crates (Tauri/GTK/iroh): fxhash, paste,
  proc-macro-error, unic-*.
