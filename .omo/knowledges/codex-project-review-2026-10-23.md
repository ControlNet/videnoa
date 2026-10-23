# Codex (gpt-6-astra) whole-project review (2026-10-23)

This was a read-only Codex review of `dev` at 88c48eb. Codex's findings come
from reading source, not from running anything. Claude re-read the code for
findings 1-3, 6, 8 and 9 and confirmed them. Findings 4, 5 and 7 have not been
re-checked yet.

None of these findings has been fixed yet.

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
