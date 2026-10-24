## What's new

- **Retries wait in the queue, and can move to another Worker.** Retrying a processing failure no longer fails with 409 when the original Worker is busy, offline or the scheduler is paused. The Controller checks that the remote job has ended, deletes the task's workspace on that Worker and queues the task for it. **Retry** in the task detail has an arrow menu listing the other enabled Workers that can run the workflow; processing, upload and rejected-submission failures can move to one of them. A 409 now shows the Controller's reason.
- **MKV outputs keep their audio.** FFmpeg could write a whole copied audio track before the first video frame, and players then lost the audio. For `.mkv`/`.mka` outputs the Worker now encodes the video on its own and muxes the source's audio, subtitles, chapters, tags and attachments with mkvmerge, keeping the source track order. Sources mkvmerge cannot read, and other containers, use the single FFmpeg pass as before.

## Fixes and reliability

Worker

- **Failed or cancelled jobs no longer publish a partial output.** The encoder is finished only when the whole pipeline succeeded, and every output is written to a hidden file beside it that replaces the output only on success. A failed or cancelled job leaves an existing output untouched, and a cancel that arrives while the output is being finalized publishes nothing.
- **An output that is the source file is rejected** before encoding, whatever the spelling, symlink or hard link.
- When mkvmerge fails, the encoded video is kept and named in the error, and mkvmerge's own message is quoted.
- Jobs writing `x.mkv` and `x.mka`, or concurrent jobs in one directory, no longer share or delete each other's temporary files.
- **Dark frames of 10-bit sources are no longer brightened.** Preprocessing guessed the range per frame, so near-black frames were brightened about 64x and dark fades flashed.
- **Portrait clips stored with a 90° rotation are no longer scrambled**, in jobs and previews.
- `PUT /api/files` streams into a hidden file and replaces the target only once the body is complete, so an interrupted upload keeps the existing file. The response is unchanged.
- The Worker WebUI no longer reopens progress connections for finished jobs.

Controller

- **Cancelling a task while it uploads or downloads no longer stops the Controller.** The transfer's stale update is retried on the next pass and the task converges on *Cancelled*.
- A second retry of the same task while the first is still running is rejected with 409, so it cannot delete the input the first retry's attempt uploads.
- The cancel confirmation in the task detail no longer stays open after selecting another task, where confirming it cancelled that task.
- Cancelling a queued task no longer trips over its failed attempt.

Build and dependencies

- The Linux Worker bundle and the Worker image include mkvmerge (MKVToolNix 101.0). The Windows bundle ships the official `mkvmerge.exe` and `mkvpropedit.exe` from MKVToolNix 101.0 and a pinned FFmpeg 8.1.3-based build.
- tokio was updated to 1.54.1 for RUSTSEC-2026-0386, and rustls, rustls-webpki, webpki-roots, webpki-root-certs and ctutils for RUSTSEC-2026-0346..0349 and RUSTSEC-2026-0381..0385.

## Downloads and Docker

Linux and Windows Worker bundles and standalone Controller archives are available below. Download all parts of a split Worker bundle before extracting the `.7z.001` file.

- Worker: `controlnet/videnoa:0.1.10`
- Controller: `controlnet/videnoa-controller:0.1.10`

Both image repositories also publish `latest`. The Controller does not require a GPU.

## Upgrade

Back up Worker configuration and Controller data before upgrading. Keep the same persistent directories when replacing containers.

- The Controller migrates its database on first start (migration 0014 adds the Worker a retried task waits for). A v0.1.9 Controller refuses to open the migrated database, so restore the backup to go back.
- **MKV outputs require mkvmerge.** The release bundles and the Worker image include it. When running from source, install the media tools with `scripts/setup_dev_media_tools.sh` or put MKVToolNix's `mkvmerge` on `PATH`; without it, MKV jobs fail before any frame is processed.
- `POST /api/tasks/{id}/retry` accepts an optional `worker_id`. `attempt_id` in its response is `null` when the retry is queued; API clients that expected an attempt ID should read the task instead.
- The Worker HTTP API is unchanged, and v0.1.10 Controllers and Workers work with v0.1.9 ones.

See the [Worker and Docker instructions](https://github.com/ControlNet/videnoa/blob/v0.1.10/README.md), [Controller archive and Docker guide](https://github.com/ControlNet/videnoa/blob/v0.1.10/README-controller.md), [Controller reference](https://github.com/ControlNet/videnoa/blob/v0.1.10/docs/controller.md) and [iroh guide](https://github.com/ControlNet/videnoa/blob/v0.1.10/docs/iroh.md).

**Full changelog:** https://github.com/ControlNet/videnoa/compare/v0.1.9...v0.1.10
