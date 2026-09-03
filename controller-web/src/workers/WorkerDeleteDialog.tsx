import { useEffect, useRef } from "react"

import type { Worker } from "../api/workerSchemas"

type WorkerDeleteDialogProps = {
  readonly worker: Worker | null
  readonly deleting: boolean
  readonly onClose: () => void
  readonly onConfirm: () => void
}

export function WorkerDeleteDialog({ worker, deleting, onClose, onConfirm }: WorkerDeleteDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null)
  const keepWorkerRef = useRef<HTMLButtonElement>(null)
  const deleteWorkerRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    const dialog = dialogRef.current
    if (worker !== null && dialog !== null && !dialog.open) {
      dialog.showModal()
      queueMicrotask(() => keepWorkerRef.current?.focus())
    }
    if (worker === null && dialog?.open === true) dialog.close()
  }, [worker])

  if (worker === null) return <dialog ref={dialogRef} className="operation-dialog" />

  return (
    <dialog
      ref={dialogRef}
      className="operation-dialog worker-delete-dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="worker-delete-title"
      aria-describedby="worker-delete-description"
      onCancel={(event) => {
        event.preventDefault()
        onClose()
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault()
          onClose()
          return
        }
        if (event.key !== "Tab") return
        const active = document.activeElement
        if (event.shiftKey && active === keepWorkerRef.current) {
          event.preventDefault()
          deleteWorkerRef.current?.focus()
        } else if (!event.shiftKey && active === deleteWorkerRef.current) {
          event.preventDefault()
          keepWorkerRef.current?.focus()
        }
      }}
    >
      <div className="operation-confirmation">
        <header>
          <p className="technical-label">DURABLE CAPACITY</p>
          <h2 id="worker-delete-title">Delete {worker.name}?</h2>
        </header>
        <p id="worker-delete-description">
          Remove the worker at <code>{worker.api_url}</code>. Durable task references block deletion; disabling the worker is the non-destructive alternative.
        </p>
        <footer>
          <button ref={keepWorkerRef} type="button" className="secondary-button" onClick={onClose}>
            Keep Worker
          </button>
          <button ref={deleteWorkerRef} type="button" className="danger-button" disabled={deleting} onClick={onConfirm}>
            {deleting ? "Deleting..." : "Delete Worker"}
          </button>
        </footer>
      </div>
    </dialog>
  )
}
