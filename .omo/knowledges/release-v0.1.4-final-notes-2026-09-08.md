# Published v0.1.4 release notes

- Release: https://github.com/ControlNet/videnoa/releases/tag/v0.1.4 (ID 384217269).
- PR #2 merged at `7efc672a606374551ea96dd224da782b85210ea0`.
- Release Workflow 34137886082 completed successfully; five release assets were present (two Controller archives and three Worker archive volumes).
- Replaced the auto-generated PR list with the final user-facing notes below. Changed only the release body; tag, title, publication state, and assets remain as published.
- Verified by reading the release back and comparing its body byte-for-byte with the submitted text.

---

## What's new

- **Iroh Worker connections:** connect Controller to a Worker using its Endpoint ID and service password, with direct connectivity where available and relay fallback. Iroh is optional; existing HTTP/HTTPS connections remain supported. Persistent identities keep Endpoint IDs stable across restarts, and Workers log their public Endpoint ID when Iroh starts.
- **Jellyfin batch naming:** add a Jellyfin-style version suffix option for batch output filenames.
- **Locally bundled fonts:** both Web UIs now include Manrope and Geist Mono fonts, removing runtime requests to Google Fonts.
- **Live Controller updates:** refresh Worker health, names, capacity, and scheduler status without manual reloads. Task filters show Worker names, and task detail refreshes preserve scrolling and expanded attempt history.

## Fixes and reliability

- **Accept updated input files:** uploads use the current file instead of rejecting changes since task creation, including changes made by media-library tools. Upload no longer repeats the admission hash; current file size is persisted for transfer verification and recovery.
- **Retry previous input-change failures:** historical upload-stage `input_changed` failures can be retried manually from task details, even if originally marked non-retryable. No database edits are required.
- **Preserve unsaved settings:** scheduler events and settings-version conflicts retain edited fields, refresh untouched fields, and prompt users to review remote changes before saving. Failed refreshes keep the draft and block updates until recovery.
- **Fix HTTP-to-Iroh password editing:** switching connection type after selecting “Clear saved password” restores the password field, allowing the saved password to be kept or replaced.
- **Prevent concurrency errors:** queued Worker updates no longer overwrite each other, and duplicate output finalizers cannot interfere with an active publication.
- **Improve Iroh lifecycle and diagnostics:** disabled Workers do not initialize discovery/relay connections, re-enabling preserves identity, and authentication failures are classified correctly. Also fix Windows dependency compatibility, reduce live-table movement, and improve MKV metadata-tool error diagnostics.

## Downloads and Docker

Linux and Windows Worker bundles and standalone Controller archives are available below. Download all parts of a split Worker bundle before extracting the `.7z.001` file.

- Worker: `controlnet/videnoa:0.1.4`
- Controller: `controlnet/videnoa-controller:0.1.4`

Both image repositories also publish `latest`. The Controller does not require a GPU.

## Upgrade

Back up existing configuration and Controller data before upgrading. Keep the same persisted directories when replacing containers; Controller database migrations run on startup. Retain the persistent Iroh identity files to preserve Endpoint IDs.

Update Controller and Workers together to use Iroh. Configure a Worker service password before enabling it. Iroh uses public discovery/relay services by default, so connectivity still depends on network reachability. Locally bundled fonts remove the font-service dependency only.

Previously failed `input_changed` tasks require an explicit **Retry** action; upgrading does not restart them automatically.

See the [Iroh setup guide](https://github.com/ControlNet/videnoa/blob/v0.1.4/docs/iroh.md), [Worker and Docker instructions](https://github.com/ControlNet/videnoa/blob/v0.1.4/README.md), and [Controller documentation](https://github.com/ControlNet/videnoa/blob/v0.1.4/README-controller.md).

**Full changelog:** https://github.com/ControlNet/videnoa/compare/v0.1.3...v0.1.4
