import { X } from "lucide-react"
import { useEffect, useRef, useState } from "react"

import type { ApiClientError } from "../api/client"
import {
  type Worker,
  type WorkerCreateRequest,
  type WorkerUpdateRequest,
  workerCreateRequestSchema,
  workerUpdateRequestSchema,
} from "../api/workerSchemas"
import { Button } from "../ui/Button"
import { CheckField, Field } from "../ui/Field"
import { workerActionMessage, workerServerFieldErrors } from "./workerErrors"

type WorkerFormDialogProps = {
  readonly worker: Worker | null
  readonly open: boolean
  readonly submitting: boolean
  readonly actionError: ApiClientError | null
  readonly onClose: () => void
  readonly onCreate: (request: WorkerCreateRequest) => Promise<boolean>
  readonly onUpdate: (worker: Worker, request: WorkerUpdateRequest) => Promise<boolean>
}

type WorkerFields = {
  readonly name: string
  readonly apiUrl: string
  readonly computeSlots: string
  readonly enabled: boolean
}

const emptyFields: WorkerFields = { name: "", apiUrl: "", computeSlots: "1", enabled: true }

export function WorkerFormDialog(props: WorkerFormDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null)
  const firstInputRef = useRef<HTMLInputElement>(null)
  const apiUrlRef = useRef<HTMLInputElement>(null)
  const computeSlotsRef = useRef<HTMLInputElement>(null)
  const [fields, setFields] = useState<WorkerFields>(() => props.worker === null ? emptyFields : {
    name: props.worker.name,
    apiUrl: props.worker.api_url,
    computeSlots: String(props.worker.compute_slots),
    enabled: props.worker.enabled,
  })
  const [fieldErrors, setFieldErrors] = useState<Partial<Record<keyof WorkerFields, string>>>({})

  useEffect(() => {
    const dialog = dialogRef.current
    if (props.open && dialog !== null && !dialog.open) {
      dialog.showModal()
      queueMicrotask(() => firstInputRef.current?.focus())
    }
    if (!props.open && dialog?.open === true) dialog.close()
  }, [props.open])

  async function submit(): Promise<void> {
    const raw = {
      name: fields.name,
      api_url: fields.apiUrl,
      enabled: fields.enabled,
      compute_slots: Number(fields.computeSlots),
    }
    const worker = props.worker
    if (worker === null) {
      const parsed = workerCreateRequestSchema.safeParse(raw)
      if (!parsed.success) {
        const errors = workerFieldErrors(parsed.error.issues)
        setFieldErrors(errors)
        if (errors.name !== undefined) firstInputRef.current?.focus()
        else if (errors.apiUrl !== undefined) apiUrlRef.current?.focus()
        else if (errors.computeSlots !== undefined) computeSlotsRef.current?.focus()
        return
      }
      if (await props.onCreate(parsed.data)) props.onClose()
      return
    }
    const parsed = workerUpdateRequestSchema.safeParse({ ...raw, version: worker.version })
    if (!parsed.success) {
      const errors = workerFieldErrors(parsed.error.issues)
      setFieldErrors(errors)
      if (errors.name !== undefined) firstInputRef.current?.focus()
      else if (errors.apiUrl !== undefined) apiUrlRef.current?.focus()
      else if (errors.computeSlots !== undefined) computeSlotsRef.current?.focus()
      return
    }
    const saved = await props.onUpdate(worker, parsed.data)
    if (saved) props.onClose()
  }

  const serverFields = workerServerFieldErrors(props.actionError)
  const nameError = fieldErrors.name ?? serverFields.name
  const urlError = fieldErrors.apiUrl ?? serverFields.apiUrl
  const slotsError = fieldErrors.computeSlots ?? serverFields.computeSlots

  useEffect(() => {
    const errors = workerServerFieldErrors(props.actionError)
    if (errors.name !== undefined) firstInputRef.current?.focus()
    else if (errors.apiUrl !== undefined) apiUrlRef.current?.focus()
    else if (errors.computeSlots !== undefined) computeSlotsRef.current?.focus()
  }, [props.actionError])

  return (
    <dialog ref={dialogRef} className="dialog operation-dialog" aria-labelledby="worker-form-title" onCancel={(event) => { event.preventDefault(); props.onClose() }}>
      <form method="dialog" className="operation-form" noValidate onSubmit={(event) => { event.preventDefault(); void submit() }}>
        <header>
          <div><h2 id="worker-form-title">{props.worker === null ? "Add Worker" : "Edit Worker"}</h2></div>
          <Button variant="outline" size="sm" icon aria-label="Close worker form" onClick={props.onClose}><X size={14} aria-hidden="true" /></Button>
        </header>
        {props.actionError === null ? null : <div className="operation-error alert alert--danger" role="alert">{workerActionMessage(props.actionError)}</div>}
        <Field ref={firstInputRef} id="worker-name" label="Worker name" name="name" autoComplete="off" spellCheck={false} value={fields.name} error={nameError} onChange={(event) => setFields({ ...fields, name: event.currentTarget.value })} />
        <Field ref={apiUrlRef} id="worker-api-url" label="Worker API URL" name="api_url" type="url" autoComplete="off" spellCheck={false} value={fields.apiUrl} error={urlError} onChange={(event) => setFields({ ...fields, apiUrl: event.currentTarget.value })} />
        <Field ref={computeSlotsRef} id="worker-slots" label="Compute slots" name="compute_slots" type="number" inputMode="numeric" min={1} max={65535} value={fields.computeSlots} error={slotsError} onChange={(event) => setFields({ ...fields, computeSlots: event.currentTarget.value })} />
        <CheckField id="worker-enabled" name="enabled" label="Enabled for scheduling" checked={fields.enabled} onChange={(enabled) => setFields({ ...fields, enabled })} />
        <footer>
          <span className="spacer" />
          <Button variant="outline" onClick={props.onClose}>Dismiss</Button>
          <Button variant="primary" type="submit" disabled={props.submitting}>{props.submitting ? "Saving..." : "Save Worker"}</Button>
        </footer>
      </form>
    </dialog>
  )
}

function workerFieldErrors(issues: readonly { readonly path: readonly PropertyKey[]; readonly message: string }[]): Partial<Record<keyof WorkerFields, string>> {
  const errors: Partial<Record<keyof WorkerFields, string>> = {}
  for (const issue of issues) {
    if (issue.path[0] === "name") errors.name = issue.message
    if (issue.path[0] === "api_url") errors.apiUrl = issue.message
    if (issue.path[0] === "compute_slots") errors.computeSlots = "Enter 1 to 65535 compute slots."
  }
  return errors
}
