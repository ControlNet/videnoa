import { Pencil, Power, PowerOff, Trash2 } from "lucide-react"
import { useRef } from "react"

import type { Worker, WorkerList } from "../api/workerSchemas"
import { formatRelative } from "../tasks/format"
import { workerHealthTone } from "../tasks/statusTone"
import { Button } from "../ui/Button"
import { Status } from "../ui/Status"

type WorkerTableProps = {
  readonly workers: WorkerList | null
  readonly loading: boolean
  readonly disabled: boolean
  readonly onEdit: (worker: Worker, trigger: HTMLButtonElement) => void
  readonly onEnabledChange: (worker: Worker, enabled: boolean) => void
  readonly onDelete: (worker: Worker, trigger: HTMLButtonElement) => void
}

export function WorkerTable(props: WorkerTableProps) {
  const frameRef = useRef<HTMLElement>(null)
  return (
    <>
      <nav className="scroll-controls worker-table-scroll-controls" aria-label="Worker table horizontal navigation">
        <p id="worker-table-scroll-hint">Scroll table to view worker capacity and actions.</p>
        <div>
          <button type="button" aria-label="Scroll worker table left" onClick={() => frameRef.current?.scrollBy({ left: -240 })}>Left</button>
          <button type="button" aria-label="Scroll worker table right" onClick={() => frameRef.current?.scrollBy({ left: 240 })}>Right</button>
        </div>
      </nav>
      {/* biome-ignore lint/a11y/noNoninteractiveTabindex: Keyboard users need direct access to horizontal overflow. */}
      <section ref={frameRef} className="scroll-frame worker-table-frame" aria-label="Scrollable worker results" aria-describedby="worker-table-scroll-hint" tabIndex={0}>
        <table className="data-table worker-table">
          <thead>
            <tr>
              <th scope="col">Name / API</th>
              <th scope="col">Health</th>
              <th scope="col">Policy</th>
              <th scope="col">Slots</th>
              <th scope="col">Tasks</th>
              <th scope="col">Transfers</th>
              <th scope="col">Last seen</th>
              <th scope="col">Last error</th>
              <th scope="col"><span className="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {props.loading && props.workers === null ? <tr><td colSpan={9} className="operation-empty">Loading worker capacity...</td></tr> : null}
            {!props.loading && props.workers?.items.length === 0 ? <tr><td colSpan={9} className="operation-empty">No workers registered. Add a worker to make scheduling capacity available.</td></tr> : null}
            {props.workers?.items.map((worker) => (
              <tr key={worker.id} className={worker.online ? "worker-online" : "worker-offline"}>
                <td className="grow-cell">
                  <span className="worker-identity">
                    <strong title={worker.name}>{worker.name}</strong>
                    <code title={worker.api_url}>{worker.api_url}</code>
                  </span>
                </td>
                <td>
                  <Status tone={workerHealthTone(worker.online)} label={worker.online ? "Online" : "Offline"} live={worker.online} />
                </td>
                <td>
                  <span className={worker.enabled ? "policy-badge policy-badge--on" : "policy-badge"}>{worker.enabled ? "Enabled" : "Disabled"}</span>
                </td>
                <td className="worker-slots">
                  <span className="numeric-cell">{worker.capacity.used_slots} / {worker.compute_slots}</span>
                  <SlotPips used={worker.capacity.used_slots} total={worker.compute_slots} online={worker.online} />
                </td>
                <td className="mono-cell">{worker.capacity.processing_tasks} processing<br />{worker.capacity.staged_tasks} staged</td>
                <td className="mono-cell">{worker.capacity.active_uploads} up / {worker.capacity.active_downloads} down</td>
                <td className="date-cell" title={worker.last_seen_at === null ? undefined : new Date(worker.last_seen_at).toLocaleString()}>
                  {worker.last_seen_at === null ? "Never" : formatRelative(worker.last_seen_at)}
                </td>
                <td className={worker.last_error === null ? "worker-error" : "worker-error worker-error--present"} title={worker.last_error ?? undefined}>{worker.last_error ?? "None"}</td>
                <td>
                  <div className="row-actions">
                    <Button variant="outline" size="sm" icon aria-label={`Edit ${worker.name}`} disabled={props.disabled} onClick={(event) => props.onEdit(worker, event.currentTarget)}>
                      <Pencil size={13} aria-hidden="true" />
                    </Button>
                    <Button variant="outline" size="sm" icon aria-label={`${worker.enabled ? "Disable" : "Enable"} ${worker.name}`} disabled={props.disabled} onClick={() => props.onEnabledChange(worker, !worker.enabled)}>
                      {worker.enabled ? <PowerOff size={13} aria-hidden="true" /> : <Power size={13} aria-hidden="true" />}
                    </Button>
                    <Button variant="outline" size="sm" icon aria-label={`Delete ${worker.name}`} disabled={props.disabled} onClick={(event) => props.onDelete(worker, event.currentTarget)}>
                      <Trash2 size={13} aria-hidden="true" />
                    </Button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </>
  )
}

/**
 * One pip per compute slot. Utilisation is the number an operator scans for, so
 * it gets a shape as well as a value.
 */
function SlotPips({ used, total, online }: { readonly used: number; readonly total: number; readonly online: boolean }) {
  if (total > 24) return null
  return (
    <span className="slot-pips" aria-hidden="true">
      {Array.from({ length: total }, (_, index) => (
        <i key={index} className={index < used ? (online ? "filled" : "filled offline") : undefined} />
      ))}
    </span>
  )
}
