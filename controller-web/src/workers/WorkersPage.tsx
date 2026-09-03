import { Plus } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import type { ApiClient } from "../api/client"
import type { Worker } from "../api/workerSchemas"
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
      <header className="operation-header"><div><p className="technical-label">DISTRIBUTED CAPACITY</p><h1>Workers</h1><p>Manage scheduling policy and inspect durable processing capacity across Videnoa nodes.</p></div><button ref={addButtonRef} type="button" className="primary-button compact-action" onClick={(event) => openForm(null, event.currentTarget)}><Plus size={16} aria-hidden="true" />Add Worker</button></header>
      {data.error === null ? null : <div className="operation-error" role="alert"><span>{data.error}</span><button type="button" onClick={data.retry}>Retry</button></div>}
      {!formOpen && data.actionError !== null ? <div className="operation-error" role="alert">{workerActionMessage(data.actionError)}</div> : null}
      <WorkerTable workers={data.workers} loading={data.loading} disabled={data.mutating} onEdit={openForm} onEnabledChange={(worker, enabled) => void data.setEnabled(worker, enabled)} onDelete={openDelete} />
      <footer className="operation-footnote"><span>{data.workers?.total.toLocaleString() ?? "--"} registered</span><span>Online health and enabled scheduling policy are independent states.</span></footer>
      <WorkerDeleteDialog worker={deletingWorker} deleting={data.mutating} onClose={closeDelete} onConfirm={() => void confirmDelete()} />
      {formOpen ? <WorkerFormDialog worker={currentEditingWorker} open submitting={data.mutating} actionError={data.actionError} onClose={closeForm} onCreate={data.createWorker} onUpdate={data.updateWorker} /> : null}
    </div>
  )
}
