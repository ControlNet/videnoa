import { RotateCcw, X } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import { type ApiClient, ApiClientError } from "../api/client"
import { cancelTaskResponseSchema, retryTaskResponseSchema } from "../api/taskSchemas"
import { formatStatus } from "./format"
import { TaskDetailContent } from "./TaskDetailContent"
import { canCancelTask, canRetryTask, failureGuidance } from "./taskActionPolicy"
import { useTaskDetail } from "./useTaskDetail"

type TaskDetailPaneProps = {
  readonly apiClient: ApiClient
  readonly taskId: string
  readonly onClose: () => void
  readonly onChanged: () => void
}

export function TaskDetailPane({ apiClient, taskId, onClose, onChanged }: TaskDetailPaneProps) {
  const paneRef = useRef<HTMLElement>(null)
  const cancelTaskRef = useRef<HTMLButtonElement>(null)
  const keepTaskRef = useRef<HTMLButtonElement>(null)
  const confirmCancelRef = useRef<HTMLButtonElement>(null)
  const data = useTaskDetail(apiClient, taskId)
  const [action, setAction] = useState<"cancel" | "retry" | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [confirmingCancel, setConfirmingCancel] = useState(false)

  useEffect(() => {
    void taskId
    paneRef.current?.focus()
  }, [taskId])
  useEffect(() => {
    if (confirmingCancel) keepTaskRef.current?.focus()
  }, [confirmingCancel])

  function dismissCancellation(): void {
    setConfirmingCancel(false)
    queueMicrotask(() => cancelTaskRef.current?.focus())
  }

  async function mutate(kind: "cancel" | "retry"): Promise<void> {
    const task = data.detail?.task
    if (task === undefined) return
    setAction(kind)
    setMessage(null)
    try {
      if (kind === "cancel") {
        await apiClient.request(`api/tasks/${task.id}/cancel`, {
          method: "POST",
          json: { version: task.version },
          schema: cancelTaskResponseSchema,
        })
      } else {
        await apiClient.request(`api/tasks/${task.id}/retry`, {
          method: "POST",
          json: { version: task.version },
          schema: retryTaskResponseSchema,
        })
      }
      setMessage(kind === "cancel" ? "Cancellation requested with the current task version." : "Retry accepted for the failed stage.")
      onChanged()
      data.reload()
    } catch (error) {
      if (!(error instanceof ApiClientError)) throw error
      if (error.status === 409) {
        setMessage(
          "The task changed before this action completed. Current detail and the bounded page were refetched; review the new state before acting again.",
        )
        onChanged()
        data.reload()
      } else if (error.code === "remote_state_ambiguous") {
        setMessage("Remote state is ambiguous. Verify the remote terminal state and task workspace manually; retry remains blocked.")
      } else if (error.code === "publication_ambiguous") {
        setMessage("Publication is ambiguous. Inspect the final and staging paths before further action; retry remains blocked.")
      } else {
        setMessage(error.message)
      }
    } finally {
      setAction(null)
    }
  }

  const task = data.detail?.task
  const guidance = task?.failure === null || task?.failure === undefined ? null : failureGuidance(task.failure.failure_code, task.failure.failure_stage)
  return (
    <section
      ref={paneRef}
      className="task-detail-pane"
      aria-label="Task Detail"
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return
        event.preventDefault()
        if (confirmingCancel) dismissCancellation()
        else onClose()
      }}
    >
      <header className="task-detail-header">
        <div>
          <p className="technical-label">SELECTED TASK</p>
          <h2>{task === undefined ? "Task Detail" : `${formatStatus(task.status)} · ${task.input_path.split("/").at(-1) ?? task.id}`}</h2>
        </div>
        <div className="task-detail-actions">
          {task !== undefined && canCancelTask(task) ? (
            <button ref={cancelTaskRef} type="button" className="danger-button" disabled={action !== null} onClick={() => setConfirmingCancel(true)}>
              Cancel Task
            </button>
          ) : null}
          {task !== undefined && canRetryTask(task) ? (
            <button type="button" className="primary-button compact-action" disabled={action !== null} onClick={() => void mutate("retry")}>
              <RotateCcw size={15} aria-hidden="true" />
              Retry Failed Stage
            </button>
          ) : null}
          <button type="button" className="icon-button" aria-label="Close Task Detail" onClick={onClose}>
            <X size={16} aria-hidden="true" />
          </button>
        </div>
      </header>
      {confirmingCancel ? (
        <div
          className="cancel-confirmation"
          role="alertdialog"
          aria-label="Confirm Task Cancellation"
          aria-modal="true"
          onKeyDown={(event) => {
            if (event.key !== "Tab") return
            const active = document.activeElement
            if (event.shiftKey && active === keepTaskRef.current) {
              event.preventDefault()
              confirmCancelRef.current?.focus()
            } else if (!event.shiftKey && active === confirmCancelRef.current) {
              event.preventDefault()
              keepTaskRef.current?.focus()
            }
          }}
        >
          <p>Cancel this task at its current stage? Completed work is preserved where the lifecycle permits.</p>
          <div>
            <button ref={keepTaskRef} type="button" className="secondary-button" onClick={dismissCancellation}>
              Keep Task
            </button>
            <button
              ref={confirmCancelRef}
              type="button"
              className="danger-button"
              onClick={() => {
                setConfirmingCancel(false)
                void mutate("cancel")
              }}
            >
              Confirm Cancellation
            </button>
          </div>
        </div>
      ) : null}
      {message === null ? null : <output className="task-action-message">{message}</output>}
      {data.error === null ? null : (
        <div className="task-action-error" role="alert">
          {data.error}{" "}
          <button type="button" onClick={data.reload}>
            Retry Detail
          </button>
        </div>
      )}
      {data.loading || data.detail === null ? (
        <p className="detail-loading">Loading task detail…</p>
      ) : (
        <TaskDetailContent detail={data.detail} guidance={guidance} />
      )}
    </section>
  )
}
