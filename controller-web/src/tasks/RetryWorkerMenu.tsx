import { useEffect, useRef, useState } from "react"

import type { ApiClient } from "../api/client"
import { type Worker, workerListSchema } from "../api/workerSchemas"

type RetryWorkerMenuProps = {
  readonly apiClient: ApiClient
  readonly workflow: string
  readonly originalWorkerId: string | null
  readonly onSelect: (worker: Worker) => void
  readonly onClose: () => void
}

type MenuState =
  | { readonly kind: "loading" }
  | { readonly kind: "failed" }
  | { readonly kind: "ready"; readonly workers: readonly Worker[] }

/**
 * Lists the other Workers a failed task can be retried on.
 *
 * The list is read when the menu opens, so capabilities are current. A Worker
 * that is disabled or does not report the workflow as runnable is shown but
 * cannot be chosen; an offline one can, and the task waits in the queue.
 */
export function RetryWorkerMenu({ apiClient, workflow, originalWorkerId, onSelect, onClose }: RetryWorkerMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null)
  const [state, setState] = useState<MenuState>({ kind: "loading" })

  useEffect(() => {
    const controller = new AbortController()
    void apiClient.request("api/workers", { schema: workerListSchema, signal: controller.signal }).then(
      (list) => {
        if (controller.signal.aborted) return
        setState({ kind: "ready", workers: list.items.filter((worker) => worker.id !== originalWorkerId) })
      },
      () => {
        if (!controller.signal.aborted) setState({ kind: "failed" })
      },
    )
    return () => controller.abort()
  }, [apiClient, originalWorkerId])

  useEffect(() => {
    if (state.kind !== "ready") return
    menuRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus()
  }, [state.kind])

  useEffect(() => {
    function closeOutside(event: PointerEvent): void {
      const parent = menuRef.current?.parentElement
      if (parent !== null && parent !== undefined && !parent.contains(event.target as Node)) onClose()
    }
    document.addEventListener("pointerdown", closeOutside)
    return () => document.removeEventListener("pointerdown", closeOutside)
  }, [onClose])

  function moveFocus(step: 1 | -1): void {
    const items = Array.from(menuRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])
    if (items.length === 0) return
    const current = items.indexOf(document.activeElement as HTMLButtonElement)
    items[(current + step + items.length) % items.length]?.focus()
  }

  return (
    <div
      ref={menuRef}
      className="retry-worker-menu"
      role="menu"
      aria-label="Retry on another Worker"
      onKeyDown={(event) => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault()
          moveFocus(event.key === "ArrowDown" ? 1 : -1)
        }
      }}
    >
      <p className="retry-worker-menu-title">Retry on another Worker</p>
      {state.kind === "loading" ? <p className="retry-worker-menu-status">Loading Workers…</p> : null}
      {state.kind === "failed" ? <p className="retry-worker-menu-status">Workers could not be loaded.</p> : null}
      {state.kind === "ready" && state.workers.length === 0 ? <p className="retry-worker-menu-status">No other Worker is registered.</p> : null}
      {state.kind === "ready"
        ? state.workers.map((worker) => {
            const reason = unavailableReason(worker, workflow)
            return (
              <button key={worker.id} type="button" role="menuitem" disabled={reason !== null} onClick={() => onSelect(worker)}>
                <span className="retry-worker-name">{worker.name}</span>
                <span className="retry-worker-note">{reason ?? (worker.online ? "Online" : "Offline · waits in queue")}</span>
              </button>
            )
          })
        : null}
    </div>
  )
}

function unavailableReason(worker: Worker, workflow: string): string | null {
  if (!worker.enabled) return "Disabled"
  const { workflows, invalid_workflows: invalid } = worker.capabilities
  if (invalid.some((item) => item.name === workflow)) return "Rejects this workflow"
  if (!workflows.some((item) => item.name === workflow)) return "Does not have this workflow"
  return null
}
