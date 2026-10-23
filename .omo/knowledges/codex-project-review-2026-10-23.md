# Codex (gpt-6-astra) whole-project review (2026-10-23)

This was a read-only Codex review of `dev` at 88c48eb. Codex's findings come
from reading source, not from running anything. Claude re-read the code for
findings 1-3, 6, 8 and 9 and confirmed them. Findings 4, 5 and 7 have not been
re-checked yet.

Findings 1-3 are fixed (see "Status" below). The others are open.

## Findings

| # | Severity | Area | Origin |
|---|---|---|---|
| 1 | High | When mkvmerge exits 2, `mux_with_mkvmerge` removes `output_path` even if that file existed before the run. | new in ead34f4 |
| 2 | High | `video_only_path()` is `.{stem}.videnoa-video.mkv`. `episode.mkv` and `episode.mka`, or two concurrent jobs, share it, and `Drop` deletes it. | new in ead34f4 |
| 3 | High | On cancel, `run_encoder_loop` does `break` and then returns `Ok(())`. When upstream fails it also returns `Ok(())`, because `blocking_recv` gets `None`. `finish()` then publishes a truncated file. With mkvmerge, the truncated video gets the full-length audio. | old (9d97b0c); mkvmerge makes it worse |
| 4 | High | Blocking pipe read/write and `wait`, plus `Command::output()` for mkvmerge, cannot be interrupted by cancel. A stalled ffmpeg or mkvmerge holds the GPU permit. | old |
| 5 | Medium | The output filter forces `setsar=1`, and the source SAR is never carried through the graph, so anamorphic sources (720x576 with SAR 16:15) come out with the wrong display aspect ratio. | old |
| 6 | Medium | `/api/jobs/{id}/ws` is registered after the `auth::protect` `route_layer`, so it is unauthenticated. It streams `NodeDebugValue` previews. You still need to know the job ID. | old |
| 7 | Medium | `transport/tunnel.rs`: unauthenticated peers can hold all 64 connection permits, because there is no connection-level auth deadline. | old |
| 8 | Medium | `package_dist.sh/.ps1` check downloaded bundles only for non-empty size and length. `setup_dev_media_tools.sh` trusts the `SHA256SUMS` inside the zip, which only proves the zip is self-consistent. | old |
| 9 | Medium | The Dockerfile pins a checksum to the mutable `misc/bin_linux64.zip`. The next asset replacement breaks rebuilds of older commits. | old pattern |

## Status

- **Fixed: findings 1-3**, without changing the controller contract below.
  - mkvmerge now writes a unique hidden temp file, which is renamed over the
    output only on success.
  - The video-only file is also unique.
  - `StreamingExecutor` finishes the encoder only after every stage succeeded
    and nothing was cancelled.
  - Details are in `video-output-non-interleaved-audio-2026-10-18.md`.
  - Finding 3 is fully fixed only for the mkvmerge path. On the direct
    FFmpeg path (MP4, or MKV that mkvmerge cannot read), a cancel now leaves
    an unfinalized partial file at the output name. Before the fix it left a
    finalized but truncated file.
  - The staged rename on Windows (`TempPath::persist`, which uses MoveFileEx
    with REPLACE_EXISTING) is not exercised by CI. The mux tests are
    `cfg(unix)`, and Windows Package smoke only runs `media_tools.ps1
    -Verify`.
- **Deliberately not changed: finding 7.** A connection-level auth deadline
  would break the controller's cached QUIC connection; see the contract
  below.
- **Open: findings 4-6, 8 and 9.**
- **Unrelated, pre-existing:**
  `streaming_executor::parallel_interpolator_tests::test_parallel_interpolator_preserves_order_and_bounds_work`
  is flaky. Its `max_active == 3` timing assertion fails about 1 run in 3 on
  unmodified code.

## Suggested fixes, from Codex

1. Mux into a temp file owned by this run, then rename it over the output only when the mux succeeds. Never delete a file this run did not create. Reject an output path that equals the source path.
2. Create the video-only temp file with a unique name and exclusive creation. Keep its path in `MuxPlan`.
3. Make the encoder loop return a distinct "cancelled" or "upstream failed" outcome. Skip `finish`/mux in that case and remove the files this run owns.
4. Have a cancel supervisor that kills the child processes, independent of the blocked I/O thread.
6. Put the WebSocket route behind auth, and check its Origin.
8 and 9. Publish immutable, versioned bundle names. Keep their digests in the repo and verify them before extracting.

## Areas Codex checked and found OK

- Controller retry persistence: version-checked updates.
- Processing recovery: identity checks.
- Linux publication: uses `RENAME_NOREPLACE`.
- Controller auth: CSRF and same-origin checks.
- Worker path validation.
- Transport: fixed loopback CONNECT target.
- Decoder EOF and exit handling.
- Windows media-tool checksums.

## What the controller relies on from the worker (checked before fixing anything)

The controller only talks to the worker through these endpoints:

- `/api/health`, `/api/workflows`, `/api/presets`;
- `/api/run` and `/api/run/validate`;
- `GET` and `DELETE` on `/api/jobs/{id}`;
- `/api/files/{path}` with GET, PUT, DELETE and `/stat`.

It uses `Authorization: Bearer <password>`. It never uses the WebSocket, `/api/fs/*` or cookies. It polls `GET /api/jobs/{id}` for progress.

Everything the worker must keep doing:

1. The job writes exactly `{task_id}/output.{ext}`, the path the controller sends in `params.output`. The controller downloads that exact name after `completed` (`controller/src/scheduler/upload.rs:134`, `download.rs:88`).
2. `GET /api/jobs/{id}` echoes `params.input`, `params.output` and `workflow_name` verbatim. Terminal job records are kept, and a 404 is treated as an ambiguous failure that will not be retried (`recovery/processing.rs`).
3. `completed` means `output.{ext}` is final. The controller downloads it immediately, never ffprobes it, and requires `size > 0` and `Content-Length == stat size`.
4. `JobStatus` is a closed set: `queued|running|completed|failed|cancelled`. Adding a value breaks the controller's parsing (`controller/src/remote/dto.rs:68`).
5. `DELETE /api/jobs/{id}` cancels the job, and `DELETE /api/files/{task_id}` removes the whole directory. 404 is accepted for both.

What the controller does not depend on:

- Every task gets a fresh `{task_id}/` directory, and an operator retry deletes it first. The controller therefore never relies on ffmpeg `-y`, an existing output, or the worker deleting a partial file.
- It never lists worker directories. Hidden temp files inside `{task_id}/` are invisible to it, and `DELETE /api/files/{task_id}` removes them with the directory.

Consequences for the findings above:

- Findings 1-3 are worker-internal. They are safe if `output.{ext}` appears only on success and the status values are unchanged.
- Finding 6: the controller does not use the WebSocket, but the web UI does.
- Finding 7: the tunnel already has a per-stream 10 s CONNECT auth deadline. The QUIC connection is cached and reused for the whole process, so a connection-level or periodic re-auth deadline would break the controller. Uploads and downloads have no total timeout.
- The tunnel's target is a separate `axum::serve` on `127.0.0.1:0` (`core/src/server/iroh.rs:131`). Timeouts added in `http_server.rs` do not reach tunnelled traffic.

## Round 2 (Codex gpt-6-astra, at 734b35f)

Codex reviewed the fix commits 55cd18e and 22060cd. It also re-checked the open
findings and looked at areas the first round covered lightly. Its full report is
not kept in the repo.

Claude verified the following by reading the code:

- **F3 (High).** No check stops the output path from being the same as the
  source path (`compile_context.rs:231`). Because the mux is now staged,
  mkvmerge can read the source and then rename the result over it, so the
  original file is replaced.
- **F4 (Medium).** In `mux_with_mkvmerge`, an early `?` drops `video_only` and
  deletes the finished encode. This happens when reserving the mux temp file
  fails, or when mkvmerge fails to launch.
- **N1 (High).** `server/files.rs:74`: an upload opens the target with
  `truncate(true)` before it streams the body. A dropped upload therefore
  destroys the existing file and leaves a partial one.
  - The fix must keep the controller contract: the `{path,size}` response, and
    a stat whose size matches counts as a finished upload.
- **N4 (High; output quality).** `infer_high_bit_source_max`
  (`super_res.rs:1100`, `frame_interpolation.rs:1093`) picks the normaliser
  for each frame from the pixel values:
  - 1023 if every sample is at most 1023;
  - otherwise 65535.

  `VideoDecoder` always pipes `rgb48le`, which uses the full 16-bit range, but
  it labels the frames `bit_depth=10`. A near-black frame from a 10-bit source,
  with every RGB value at most 1023/65535 (about 8-bit 0-3), is therefore
  normalised by 1023 and comes out about 64x brighter. Dark fades can flash.

Findings reported by Codex but not yet re-checked by Claude:

| ID | Severity | Finding |
|---|---|---|
| F1 | High | The direct FFmpeg path still truncates an existing output before the job succeeds. |
| F2 | High | A cancel that arrives during the finish or mux step does not stop the result from being published. Same root cause as finding 4. |
| N2 | High | `downloader.rs`: a remote filename can overwrite an existing file, and the `.part` file is not unique. |
| N3 | Medium | Rotated sources: FFmpeg's autorotate swaps the width and height, but the frames keep the probed dimensions. |
| N5 | Medium | The files API has a TOCTOU symlink race on parent directories. It needs a local attacker. |
| N6 | Medium | `web/src/api/client.ts`: unsubscribing does not clear a pending WebSocket reconnect timer. |

Open findings 4, 5, 6, 8 and 9 were confirmed as still present.
