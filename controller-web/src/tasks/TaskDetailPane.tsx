import { ChevronDown, RotateCcw, X } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import { type ApiClient, ApiClientError } from "../api/client"
import { Button } from "../ui/Button"
import { cancelTaskResponseSchema, retryTaskResponseSchema } from "../api/taskSchemas"
import type { Worker } from "../api/workerSchemas"
import { formatStatus } from "./format"
import { RetryWorkerMenu } from "./RetryWorkerMenu"
import { TaskDetailContent } from "./TaskDetailContent"
import {
  canAbandonTask,
  canCancelTask,
  canDiscardTask,
  canRetryOnAnotherWorker,
  canRetryTask,
  failureGuidance,
} from "./taskActionPolicy"
import { abandonTaskResponseSchema, discardTaskResponseSchema } from "./taskActionSchemas"
import { useTaskDetail } from "./useTaskDetail"

type TaskAction = "cancel" | "retry" | "abandon" | "discard"
type ConfirmedAction = Exclude<TaskAction, "retry">

const confirmations: Record<ConfirmedAction, { label: string; question: string; keep: string; confirm: string }> = {
  cancel: {
    label: "Confirm Task Cancellation",
    question: "Cancel this task at its current stage? Completed work is preserved where the lifecycle permits.",
    keep: "Keep Task",
    confirm: "Confirm Cancellation",
  },
  abandon: {
    label: "Confirm Task Abandon",
    question:
      "Stop waiting for the Worker? The task fails although the Worker has not confirmed that its job stopped or that its task folder was deleted. Check that Worker before using it again.",
    keep: "Keep Waiting",
    confirm: "Confirm Abandon",
  },
  discard: {
    label: "Confirm Task Discard",
    question:
      "Discard this failed task? It can no longer be retried. Its temporary files are deleted, and its folder on the Worker is deleted when the Worker confirms the job ended. The published output is not touched.",
    keep: "Keep Task",
    confirm: "Confirm Discard",
  },
}

type TaskDetailPaneProps = {
  readonly apiClient: ApiClient
  readonly taskId: string
  readonly onClose: () => void
  readonly onChanged: () => void
}

/*
 * Keyed by task so a pending cancel confirmation, an open Worker menu or a late
 * action result for one task never carries over to the next selection, where a
 * carried-over confirmation would cancel a task the operator never confirmed.
 */
export function TaskDetailPane(props: TaskDetailPaneProps) {
  return <TaskDetailPaneBody key={props.taskId} {...props} />
}

function TaskDetailPaneBody({ apiClient, taskId, onClose, onChanged }: TaskDetailPaneProps) {
  const paneRef = useRef<HTMLElement>(null)
  const cancelTaskRef = useRef<HTMLButtonElement>(null)
  const abandonTaskRef = useRef<HTMLButtonElement>(null)
  const discardTaskRef = useRef<HTMLButtonElement>(null)
  const keepTaskRef = useRef<HTMLButtonElement>(null)
  const confirmActionRef = useRef<HTMLButtonElement>(null)
  const data = useTaskDetail(apiClient, taskId)
  const [action, setAction] = useState<TaskAction | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [confirming, setConfirming] = useState<ConfirmedAction | null>(null)
  const [choosingWorker, setChoosingWorker] = useState(false)
  const retryMenuRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    void taskId
    paneRef.current?.focus()
  }, [taskId])
  useEffect(() => {
    if (confirming !== null) keepTaskRef.current?.focus()
  }, [confirming])

  function dismissConfirmation(): void {
    if (confirming === null) return
    const trigger = { cancel: cancelTaskRef, abandon: abandonTaskRef, discard: discardTaskRef }[confirming]
    setConfirming(null)
    queueMicrotask(() => trigger.current?.focus())
  }

  function closeWorkerMenu(): void {
    setChoosingWorker(false)
    queueMicrotask(() => retryMenuRef.current?.focus())
  }

  async function mutate(kind: TaskAction, worker?: Worker): Promise<void> {
    const task = data.detail?.task
    if (task === undefined) return
    setAction(kind)
    setChoosingWorker(false)
    setMessage(null)
    try {
      if (kind === "cancel") {
        await apiClient.request(`api/tasks/${task.id}/cancel`, {
          method: "POST",
          json: { version: task.version },
          schema: cancelTaskResponseSchema,
        })
        setMessage("Cancellation requested with the current task version.")
      } else if (kind === "abandon") {
        await apiClient.request(`api/tasks/${task.id}/abandon`, {
          method: "POST",
          json: { version: task.version },
          schema: abandonTaskResponseSchema,
        })
        setMessage("Task abandoned. The Worker has not confirmed that its job stopped; check it before using it again.")
      } else if (kind === "discard") {
        const discarded = await apiClient.request(`api/tasks/${task.id}/discard`, {
          method: "POST",
          json: { version: task.version },
          schema: discardTaskResponseSchema,
        })
        setMessage(
          discarded.worker_folder_deleted
            ? "Task discarded. Its temporary files and Worker folder were deleted."
            : "Task discarded. Its temporary files were deleted; its Worker folder was kept because the Worker could not confirm that the job ended.",
        )
      } else {
        const retried = await apiClient.request(`api/tasks/${task.id}/retry`, {
          method: "POST",
          json: worker === undefined ? { version: task.version } : { version: task.version, worker_id: worker.id },
          schema: retryTaskResponseSchema,
        })
        setMessage(retryMessage(retried.status, worker))
      }
      onChanged()
      data.reload()
    } catch (error) {
      if (!(error instanceof ApiClientError)) throw error
      if (error.status === 409) {
        setMessage(`${conflictMessage(error.message)} Current detail and the bounded page were refetched; review the new state before acting again.`)
        onChanged()
        data.reload()
      } else if (error.code === "remote_state_ambiguous") {
        setMessage("Remote state is ambiguous. Verify the remote terminal state and task workspace manually; retry remains blocked.")
      } else if (error.code === "publication_ambiguous") {
        setMessage("Publication is ambiguous. Inspect the final and staging paths before further action; retry remains blocked.")
      } else {
        setMessage(error.fieldErrors[0]?.message ?? error.message)
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
        if (choosingWorker) closeWorkerMenu()
        else if (confirming !== null) dismissConfirmation()
        else onClose()
      }}
    >
      <header className="task-detail-header">
        <div>
          <h2>{task === undefined ? "Task Detail" : `${formatStatus(task.status)} · ${task.input_path.split("/").at(-1) ?? task.id}`}</h2>
        </div>
        <div className="task-detail-actions">
          {task !== undefined && canCancelTask(task) ? (
            <Button ref={cancelTaskRef} variant="danger" size="sm" disabled={action !== null} onClick={() => setConfirming("cancel")}>
              Cancel Task
            </Button>
          ) : null}
          {task !== undefined && canAbandonTask(task) ? (
            <Button
              ref={abandonTaskRef}
              variant="danger"
              size="sm"
              disabled={action !== null}
              title="Stop waiting for a Worker that cannot confirm the cancellation"
              onClick={() => setConfirming("abandon")}
            >
              Abandon Task
            </Button>
          ) : null}
          {task !== undefined && canRetryTask(task) ? (
            <div className="retry-split">
              <Button variant="primary" size="sm" disabled={action !== null} title="Retry the failed stage" onClick={() => void mutate("retry")}>
                <RotateCcw size={13} aria-hidden="true" />
                Retry
              </Button>
              {canRetryOnAnotherWorker(task) ? (
                <Button
                  ref={retryMenuRef}
                  variant="primary"
                  size="sm"
                  icon
                  aria-label="Retry on another Worker"
                  aria-haspopup="menu"
                  aria-expanded={choosingWorker}
                  disabled={action !== null}
                  onClick={() => setChoosingWorker((open) => !open)}
                >
                  <ChevronDown size={13} aria-hidden="true" />
                </Button>
              ) : null}
              {choosingWorker ? (
                <RetryWorkerMenu
                  apiClient={apiClient}
                  workflow={task.workflow}
                  originalWorkerId={task.worker_id}
                  onSelect={(worker) => void mutate("retry", worker)}
                  onClose={() => setChoosingWorker(false)}
                />
              ) : null}
            </div>
          ) : null}
          {task !== undefined && canDiscardTask(task) ? (
            <Button
              ref={discardTaskRef}
              variant="outline"
              size="sm"
              disabled={action !== null}
              title="Give up on this task and release its files"
              onClick={() => setConfirming("discard")}
            >
              Discard Task
            </Button>
          ) : null}
          <Button variant="outline" size="sm" icon aria-label="Close Task Detail" onClick={onClose}>
            <X size={14} aria-hidden="true" />
          </Button>
        </div>
      </header>
      {confirming === null ? null : (
        <div
          className="cancel-confirmation"
          role="alertdialog"
          aria-label={confirmations[confirming].label}
          aria-modal="true"
          onKeyDown={(event) => {
            if (event.key !== "Tab") return
            const active = document.activeElement
            if (event.shiftKey && active === keepTaskRef.current) {
              event.preventDefault()
              confirmActionRef.current?.focus()
            } else if (!event.shiftKey && active === confirmActionRef.current) {
              event.preventDefault()
              keepTaskRef.current?.focus()
            }
          }}
        >
          <p>{confirmations[confirming].question}</p>
          <div>
            <Button ref={keepTaskRef} variant="outline" size="sm" onClick={dismissConfirmation}>
              {confirmations[confirming].keep}
            </Button>
            <Button
              ref={confirmActionRef}
              variant="danger"
              size="sm"
              onClick={() => {
                setConfirming(null)
                void mutate(confirming)
              }}
            >
              {confirmations[confirming].confirm}
            </Button>
          </div>
        </div>
      )}
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
        <TaskDetailContent
          detail={data.detail}
          guidance={guidance}
          loadingMore={data.loadingMore}
          onLoadMore={data.loadMore}
        />
      )}
    </section>
  )
}

function retryMessage(status: string, worker: Worker | undefined): string {
  if (status !== "queued") return "Retry accepted for the failed stage."
  return worker === undefined
    ? "Retry queued. The task starts when its Worker has a free slot."
    : `Retry queued for ${worker.name}. The original Worker's workspace for this task was deleted.`
}

function conflictMessage(serverMessage: string): string {
  if (serverMessage === "task changed since it was read") return "The task changed before this action completed."
  return `The Controller refused the action: ${serverMessage}.`
}
