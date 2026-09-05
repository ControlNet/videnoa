import { Plus, RotateCcw, X } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import { type ApiClient, ApiClientError } from "../api/client"
import { type Task, type TaskCreateRequest, taskCreateResponseSchema } from "../api/taskSchemas"
import { ManualTaskField } from "./ManualTaskField"
import { type ManualTaskFields, manualTaskErrorMessage, manualTaskFieldErrors } from "./manualTaskForm"
import { beginSubmission, markSubmissionAmbiguous, type SubmissionIntent } from "./submissionIntent"

type ManualTaskDialogProps = {
  readonly apiClient: ApiClient
  readonly open: boolean
  readonly onClose: () => void
  readonly onCreated: (task: Task) => void
}

const emptyFields: ManualTaskFields = { inputPath: "", outputPath: "", workflow: "", priority: "0" }

export function ManualTaskDialog({ apiClient, open, onClose, onCreated }: ManualTaskDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const outputRef = useRef<HTMLInputElement>(null)
  const workflowRef = useRef<HTMLInputElement>(null)
  const priorityRef = useRef<HTMLInputElement>(null)
  const [fields, setFields] = useState<ManualTaskFields>(emptyFields)
  const [fieldErrors, setFieldErrors] = useState<Partial<Record<keyof ManualTaskFields, string>>>({})
  const [serverError, setServerError] = useState<string | null>(null)
  const [intent, setIntent] = useState<SubmissionIntent | null>(null)
  const [submitting, setSubmitting] = useState(false)

  useEffect(() => {
    const dialog = dialogRef.current
    if (open && dialog !== null && !dialog.open) {
      dialog.showModal()
      inputRef.current?.focus()
    }
    if (!open && dialog?.open === true) dialog.close()
  }, [open])

  function close(): void {
    if (submitting) return
    setFields(emptyFields)
    setFieldErrors({})
    setServerError(null)
    setIntent(null)
    onClose()
  }

  function updateFields(next: ManualTaskFields): void {
    if (intent?.state === "ambiguous") setIntent(null)
    setFields(next)
  }

  async function submit(): Promise<void> {
    const parsedPriority = Number(fields.priority)
    const errors: Partial<Record<keyof ManualTaskFields, string>> = {}
    if (fields.inputPath === "") errors.inputPath = "Enter the exact input path."
    if (fields.outputPath === "") errors.outputPath = "Enter the exact output path."
    if (fields.workflow === "") errors.workflow = "Enter the workflow name."
    if (fields.priority === "" || !Number.isInteger(parsedPriority)) errors.priority = "Priority must be a whole number."
    setFieldErrors(errors)
    const firstErrorRef =
      errors.inputPath !== undefined
        ? inputRef
        : errors.outputPath !== undefined
          ? outputRef
          : errors.workflow !== undefined
            ? workflowRef
            : errors.priority !== undefined
              ? priorityRef
              : null
    if (firstErrorRef !== null) {
      firstErrorRef.current?.focus()
      return
    }

    const body: TaskCreateRequest = {
      input_path: fields.inputPath,
      output_path: fields.outputPath,
      workflow: fields.workflow,
      priority: parsedPriority,
      source: "manual",
      source_reference: null,
    }
    const nextIntent = beginSubmission(intent, body)
    setIntent(nextIntent)
    setSubmitting(true)
    setServerError(null)
    try {
      const created = await apiClient.request("api/tasks", {
        method: "POST",
        headers: { "Idempotency-Key": nextIntent.key },
        json: nextIntent.body,
        schema: taskCreateResponseSchema,
      })
      onCreated(created)
      setFields(emptyFields)
      setIntent(null)
      onClose()
    } catch (error) {
      if (!(error instanceof ApiClientError)) throw error
      if (error.code === "network_failure") {
        setIntent(markSubmissionAmbiguous(nextIntent))
        setServerError("The response was dropped, so creation is uncertain. Retry Same Task to replay the identical request with the same key.")
      } else {
        setIntent(null)
        setServerError(manualTaskErrorMessage(error))
        const serverFieldErrors = manualTaskFieldErrors(error)
        setFieldErrors(serverFieldErrors)
        queueMicrotask(() => {
          if (serverFieldErrors.inputPath !== undefined) inputRef.current?.focus()
          else if (serverFieldErrors.outputPath !== undefined) outputRef.current?.focus()
          else if (serverFieldErrors.workflow !== undefined) workflowRef.current?.focus()
          else if (serverFieldErrors.priority !== undefined) priorityRef.current?.focus()
        })
      }
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <dialog
      ref={dialogRef}
      className="task-dialog"
      aria-labelledby="add-task-title"
      onCancel={(event) => {
        event.preventDefault()
        close()
      }}
    >
      <form
        method="dialog"
        className="task-create-form"
        onSubmit={(event) => {
          event.preventDefault()
          void submit()
        }}
      >
        <header>
          <div>
            <p className="technical-label">MANUAL INTAKE</p>
            <h2 id="add-task-title">Add Task</h2>
          </div>
          <button type="button" className="icon-button" aria-label="Close Add Task" onClick={close}>
            <X size={16} aria-hidden="true" />
          </button>
        </header>
        <p>Paths are submitted exactly as entered. Retry never renames output or changes task paths.</p>
        {serverError === null ? null : (
          <div className="task-action-error" role="alert">
            {serverError}
          </div>
        )}
        <ManualTaskField
          label="Input Path"
          name="input_path"
          value={fields.inputPath}
          error={fieldErrors.inputPath}
          inputRef={inputRef}
          onChange={(inputPath) => updateFields({ ...fields, inputPath })}
        />
        <ManualTaskField
          label="Output Path"
          name="output_path"
          value={fields.outputPath}
          error={fieldErrors.outputPath}
          inputRef={outputRef}
          onChange={(outputPath) => updateFields({ ...fields, outputPath })}
        />
        <ManualTaskField
          label="Workflow"
          name="workflow"
          value={fields.workflow}
          error={fieldErrors.workflow}
          inputRef={workflowRef}
          onChange={(workflow) => updateFields({ ...fields, workflow })}
        />
        <label className="task-form-field" htmlFor="task-priority">
          <span>Priority</span>
          <input
            ref={priorityRef}
            id="task-priority"
            name="priority"
            type="number"
            inputMode="numeric"
            autoComplete="off"
            spellCheck={false}
            value={fields.priority}
            aria-invalid={fieldErrors.priority === undefined ? undefined : true}
            aria-describedby={fieldErrors.priority === undefined ? undefined : "task-priority-error"}
            onChange={(event) => updateFields({ ...fields, priority: event.currentTarget.value })}
          />
          {fieldErrors.priority === undefined ? null : <small id="task-priority-error">{fieldErrors.priority}</small>}
        </label>
        <footer>
          <button type="button" className="secondary-button" onClick={close}>
            Dismiss
          </button>
          <button type="submit" className="primary-button task-create-submit" disabled={submitting}>
            {intent?.state === "ambiguous" ? <RotateCcw size={16} aria-hidden="true" /> : <Plus size={16} aria-hidden="true" />}
            {submitting ? "Submitting…" : intent?.state === "ambiguous" ? "Retry Same Task" : "Create Task"}
          </button>
        </footer>
      </form>
    </dialog>
  )
}
