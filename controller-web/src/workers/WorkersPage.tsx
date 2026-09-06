import { Plus } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import type { ApiClient } from "../api/client"
import type { Worker, WorkerList } from "../api/workerSchemas"
import { Button } from "../ui/Button"
import { type Counter, Counters } from "../ui/Counters"
import "../operations.css"
import { useWorkersData } from "./useWorkersData"
import { WorkerDeleteDialog } from "./WorkerDeleteDialog"
import { WorkerFormDialog } from "./WorkerFormDialog"
import { WorkerTable } from "./WorkerTable"
import { workerActionMessage } from "./workerErrors"

type WorkersPageProps = { readonly apiClient: ApiClient }

export function WorkersPage({ apiClient }: WorkersPageProps) {
  const data = useWorkersData(apiClient)
  const addButtonRef = useRef<HTMLButtonElement>(null)
  const formButtonRef = useRef<HTMLButtonElement | null>(null)
  const deleteButtonRef = useRef<HTMLButtonElement | null>(null)
  const confirmedDeleteFocusRef = useRef<"add" | "trigger" | null>(null)
  const [confirmedDeleteGeneration, setConfirmedDeleteGeneration] = useState(0)
  const [formOpen, setFormOpen] = useState(false)
  const [editingWorker, setEditingWorker] = useState<Worker | null>(null)
  const [deletingWorker, setDeletingWorker] = useState<Worker | null>(null)
  const currentEditingWorker = editingWorker === null
    ? null
    : data.workers?.items.find((worker) => worker.id === editingWorker.id) ?? editingWorker

  useEffect(() => {
    void confirmedDeleteGeneration
    if (data.mutating || deletingWorker !== null) return
    const target = confirmedDeleteFocusRef.current
    confirmedDeleteFocusRef.current = null
    if (target === "add") addButtonRef.current?.focus()
    if (target === "trigger") deleteButtonRef.current?.focus()
  }, [confirmedDeleteGeneration, data.mutating, deletingWorker])

  function openForm(worker: Worker | null, trigger: HTMLButtonElement): void {
    data.clearActionError()
    formButtonRef.current = trigger
    setEditingWorker(worker)
    setFormOpen(true)
  }

  function closeForm(): void {
    setFormOpen(false)
    setEditingWorker(null)
    queueMicrotask(() => formButtonRef.current?.focus())
  }

  function openDelete(worker: Worker, trigger: HTMLButtonElement): void {
    data.clearActionError()
    deleteButtonRef.current = trigger
    setDeletingWorker(worker)
  }

  function closeDelete(): void {
    setDeletingWorker(null)
    queueMicrotask(() => deleteButtonRef.current?.focus())
  }

  async function confirmDelete(): Promise<void> {
    if (deletingWorker === null) return
    const worker = deletingWorker
    setDeletingWorker(null)
    const deleted = await data.deleteWorker(worker)
    confirmedDeleteFocusRef.current = deleted ? "add" : "trigger"
    setConfirmedDeleteGeneration((generation) => generation + 1)
  }

  return (
    <div className="route-page operation-page">
      <div className="command-row">
        <h1>Workers</h1>
        <span className="spacer" />
        <Button ref={addButtonRef} variant="primary" onClick={(event) => openForm(null, event.currentTarget)}>
          <Plus size={13} strokeWidth={2.4} aria-hidden="true" />
          Add Worker
        </Button>
      </div>
      {data.error === null ? null : <div className="operation-error alert alert--danger" role="alert"><span>{data.error}</span><button type="button" onClick={data.retry}>Retry</button></div>}
      {!formOpen && data.actionError !== null ? <div className="operation-error alert alert--danger" role="alert">{workerActionMessage(data.actionError)}</div> : null}
      <div className="stats-row">
        <Counters label="Worker capacity" counters={workerCounters(data.workers)} />
      </div>
      <div className="route-body">
        <WorkerTable workers={data.workers} loading={data.loading} disabled={data.mutating} onEdit={openForm} onEnabledChange={(worker, enabled) => void data.setEnabled(worker, enabled)} onDelete={openDelete} />
      </div>
      <WorkerDeleteDialog worker={deletingWorker} deleting={data.mutating} onClose={closeDelete} onConfirm={() => void confirmDelete()} />
      {formOpen ? <WorkerFormDialog worker={currentEditingWorker} open submitting={data.mutating} actionError={data.actionError} onClose={closeForm} onCreate={data.createWorker} onUpdate={data.updateWorker} /> : null}
    </div>
  )
}

function workerCounters(workers: WorkerList | null): readonly Counter[] {
  if (workers === null) {
    return [
      { label: "All", value: null, tone: "total" },
      { label: "Online", value: null, tone: "positive" },
      { label: "Offline", value: null, tone: "negative" },
      { label: "Enabled", value: null, tone: "quiet" },
      { label: "Slots", value: null, tone: "active" },
    ]
  }
  const online = workers.items.filter((worker) => worker.online).length
  const enabled = workers.items.filter((worker) => worker.enabled).length
  const slots = workers.items.reduce((total, worker) => total + worker.compute_slots, 0)
  const used = workers.items.reduce((total, worker) => total + worker.capacity.used_slots, 0)
  return [
    { label: "All", value: workers.total.toLocaleString(), tone: "total" },
    { label: "Online", value: online.toLocaleString(), tone: "positive" },
    { label: "Offline", value: (workers.items.length - online).toLocaleString(), tone: "negative" },
    { label: "Enabled", value: enabled.toLocaleString(), tone: "quiet" },
    { label: "Slots", value: `${used.toLocaleString()} / ${slots.toLocaleString()}`, tone: "active" },
  ]
}
