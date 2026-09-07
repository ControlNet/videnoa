# Worker relay log while reportedly disabled (2026-09-07)

The user reported `home is now relay ... was None` from a Worker while iroh appeared disabled. This log alone does not prove that a disabled Worker created a new endpoint.

Read-only inspection of nectar3 found one `videnoa` process (PID 531142), working directory `/home/zhixi/videnoa`, executable `/home/zhixi/videnoa/videnoa`. At inspection time, `/home/zhixi/videnoa/data/config.toml` contained `[iroh] enabled = true`. This establishes the current persisted setting only, not the setting at the reported log's time or whether that log originated on this host. No process or configuration was modified.

Code findings:

- Worker `AppState::reconcile_iroh` returns without starting an endpoint when disabled and shuts down any existing server using `Server::shutdown`, which awaits `endpoint.close()`.
- `/api/iroh` opens the local persistent `Identity` to show the Endpoint ID, without binding a network endpoint.
- The WebUI checkbox edits form state; the settings Save action sends `PUT /api/config`, which persists the setting and reconciles the runtime. Toggling alone does not apply the change. The existing UI description states this.
- In iroh 1.1.0, `socket/transports/relay/actor.rs::on_network_change` emits this log on selecting a preferred home relay and then starts/updates the relay connection. It is not proof of an incoming Controller connection or a successfully established relay connection.

Confirm host and whether settings were saved if the discrepancy persists. No transport defect has been established and no transport code was changed for this report.
