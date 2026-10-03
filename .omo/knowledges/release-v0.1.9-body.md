## What's new

- **Self-hosted iroh relays.** Workers and the Controller can use your own [iroh relay](https://github.com/ControlNet/videnoa/blob/v0.1.9/docs/iroh.md#self-hosted-relays) instead of the public N0 infrastructure. Set the relay URLs under **Settings** in either WebUI, or `[iroh] relay_urls` in `config.toml` / `controller.toml`. With relays configured, nothing is published to or looked up from N0's address service, and the Controller dials a Worker's Endpoint ID through the listed relays. Several relays can be listed; an unreachable one falls back to another.
- **Optional public relays next to your own.** *Also use the public iroh relays* (`use_public_relays = true`) adds N0's public relays to the self-hosted ones. They are chosen by latency alongside yours rather than held in reserve, and public address lookup stays off.
- **Workflows a Worker cannot run are rejected when a task is created.** Workers validate their workflows on each capability refresh; the Controller refuses tasks and batches whose workflow every Worker rejects, and the Workers page shows the rejected workflows with the Worker's reason.
- **Rejected submissions can be retried.** A task whose submission the Worker rejected can be retried manually; the uploaded input is submitted again without re-uploading.
- **Faster previews of long videos.** Preview frames are read by seeking to each sample instead of decoding the video up to the last one. On a 23.7-minute 1080p HEVC file, 10 samples took 3.7 s instead of 78.5 s, and long 4K sources no longer exceed the extraction time limit.

## Fixes and reliability

Worker

- **Downloads are isolated per job.** Downloads with the same file name no longer overwrite each other within a workflow, across concurrent jobs or across CLI runs. Each job downloads into its own directory, which is deleted when the job ends.
- `POST /api/run/validate` checks a workflow the way `POST /api/run` does without creating a job (204, or 400 with the same error).
- Changing the relays restarts the iroh endpoint with the same Endpoint ID.

Controller

- **Newer Workers stay online.** Unknown fields in Worker responses are ignored instead of failing the capability refresh and taking the Worker offline.
- Saved Worker workflows are submitted by name, so workflows listed as `name.json` run.
- Saved iroh relays apply when the Controller restarts; Settings shows *Restart required* without locking the scheduler controls.

CI

- The Worker web app's lint and tests run in CI. Dependabot is disabled.

## Downloads and Docker

Linux and Windows Worker bundles and standalone Controller archives are available below. Download all parts of a split Worker bundle before extracting the `.7z.001` file.

- Worker: `controlnet/videnoa:0.1.9`
- Controller: `controlnet/videnoa-controller:0.1.9`

Both image repositories also publish `latest`. The Controller does not require a GPU.

## Upgrade

Back up Worker configuration and Controller data before upgrading. Keep the same persistent directories when replacing containers.

- Existing configurations keep using the public N0 relays; nothing changes until relays are configured.
- With self-hosted relays, the Worker and the Controller must list the same relays and make the same public-relay choice, otherwise they cannot reach each other.
- Tasks whose workflow every Worker rejects now fail at creation with a `workflow` field error instead of failing after submission. Workers older than v0.1.9 do not report validation (404), so their workflows stay eligible.
- A v0.1.9 Controller works with v0.1.8 Workers, and v0.1.9 Workers keep the response fields older Controllers require.
- Files downloaded by a job are deleted when the job ends; keep a copy elsewhere if a workflow relied on them remaining in the system temporary directory.

See the [Worker and Docker instructions](https://github.com/ControlNet/videnoa/blob/v0.1.9/README.md), [Controller archive and Docker guide](https://github.com/ControlNet/videnoa/blob/v0.1.9/README-controller.md), [Controller reference](https://github.com/ControlNet/videnoa/blob/v0.1.9/docs/controller.md) and [iroh guide](https://github.com/ControlNet/videnoa/blob/v0.1.9/docs/iroh.md).

**Full changelog:** https://github.com/ControlNet/videnoa/compare/v0.1.8...v0.1.9
