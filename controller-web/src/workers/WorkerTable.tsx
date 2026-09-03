import { Pencil, Power, PowerOff, Trash2 } from "lucide-react"

import type { Worker, WorkerList } from "../api/workerSchemas"

type WorkerTableProps = {
  readonly workers: WorkerList | null
  readonly loading: boolean
  readonly disabled: boolean
  readonly onEdit: (worker: Worker) => void
  readonly onEnabledChange: (worker: Worker, enabled: boolean) => void
  readonly onDelete: (worker: Worker, trigger: HTMLButtonElement) => void
}

export function WorkerTable(props: WorkerTableProps) {
  return (
    // biome-ignore lint/a11y/noNoninteractiveTabindex: Keyboard users need direct access to horizontal overflow.
    <section className="worker-table-frame" aria-label="Scrollable worker results" tabIndex={0}>
      <table className="worker-table">
        <thead><tr><th scope="col">Name / API</th><th scope="col">Health</th><th scope="col">Policy</th><th scope="col">Slots</th><th scope="col">Tasks</th><th scope="col">Transfers</th><th scope="col">Last seen</th><th scope="col">Last error</th><th scope="col">Actions</th></tr></thead>
        <tbody>
          {props.loading && props.workers === null ? <tr><td colSpan={9} className="operation-empty">Loading worker capacity...</td></tr> : null}
          {!props.loading && props.workers?.items.length === 0 ? <tr><td colSpan={9} className="operation-empty">No workers registered. Add a worker to make scheduling capacity available.</td></tr> : null}
          {props.workers?.items.map((worker) => (
            <tr key={worker.id}>
              <td className="worker-identity"><strong title={worker.name}>{worker.name}</strong><code title={worker.api_url}>{worker.api_url}</code></td>
              <td><span className={`operation-status ${worker.online ? "healthy" : "offline"}`}>{worker.online ? "Online" : "Offline"}</span></td>
              <td><span className={`operation-status ${worker.enabled ? "enabled" : "disabled"}`}>{worker.enabled ? "Enabled" : "Disabled"}</span></td>
              <td className="numeric-cell">{worker.capacity.used_slots} / {worker.compute_slots}</td>
              <td className="mono-cell">{worker.capacity.processing_tasks} processing<br />{worker.capacity.staged_tasks} staged</td>
              <td className="mono-cell">{worker.capacity.active_uploads} up / {worker.capacity.active_downloads} down</td>
              <td className="date-cell">{formatDate(worker.last_seen_at)}</td>
              <td className="worker-error" title={worker.last_error ?? undefined}>{worker.last_error ?? "None"}</td>
              <td><div className="row-actions"><button type="button" aria-label={`Edit ${worker.name}`} disabled={props.disabled} onClick={() => props.onEdit(worker)}><Pencil size={14} aria-hidden="true" /></button><button type="button" aria-label={`${worker.enabled ? "Disable" : "Enable"} ${worker.name}`} disabled={props.disabled} onClick={() => props.onEnabledChange(worker, !worker.enabled)}>{worker.enabled ? <PowerOff size={14} aria-hidden="true" /> : <Power size={14} aria-hidden="true" />}</button><button type="button" aria-label={`Delete ${worker.name}`} disabled={props.disabled} onClick={(event) => props.onDelete(worker, event.currentTarget)}><Trash2 size={14} aria-hidden="true" /></button></div></td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  )
}

function formatDate(value: string | null): string {
  return value === null ? "Never" : new Date(value).toLocaleString()
}
