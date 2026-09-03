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
    <dialog ref={dialogRef} className="operation-dialog" aria-labelledby="worker-form-title" onCancel={(event) => { event.preventDefault(); props.onClose() }}>
      <form method="dialog" className="operation-form" noValidate onSubmit={(event) => { event.preventDefault(); void submit() }}>
        <header>
          <div><p className="technical-label">CAPACITY REGISTRY</p><h2 id="worker-form-title">{props.worker === null ? "Add Worker" : "Edit Worker"}</h2></div>
          <button type="button" className="icon-button" aria-label="Close worker form" onClick={props.onClose}><X size={16} aria-hidden="true" /></button>
        </header>
        {props.actionError === null ? null : <div className="operation-error" role="alert">{workerActionMessage(props.actionError)}</div>}
        <label className="operation-field" htmlFor="worker-name"><span>Worker name</span><input ref={firstInputRef} id="worker-name" name="name" autoComplete="off" spellCheck={false} value={fields.name} aria-invalid={nameError === undefined ? undefined : true} aria-describedby={nameError === undefined ? undefined : "worker-name-error"} onChange={(event) => setFields({ ...fields, name: event.currentTarget.value })} />{nameError === undefined ? null : <small id="worker-name-error" role="alert">{nameError}</small>}</label>
        <label className="operation-field" htmlFor="worker-api-url"><span>Worker API URL</span><input ref={apiUrlRef} id="worker-api-url" name="api_url" type="url" autoComplete="off" spellCheck={false} value={fields.apiUrl} aria-invalid={urlError === undefined ? undefined : true} aria-describedby={urlError === undefined ? undefined : "worker-api-url-error"} onChange={(event) => setFields({ ...fields, apiUrl: event.currentTarget.value })} />{urlError === undefined ? null : <small id="worker-api-url-error" role="alert">{urlError}</small>}</label>
        <label className="operation-field" htmlFor="worker-slots"><span>Compute slots</span><input ref={computeSlotsRef} id="worker-slots" name="compute_slots" type="number" inputMode="numeric" min="1" max="65535" value={fields.computeSlots} aria-invalid={slotsError === undefined ? undefined : true} aria-describedby={slotsError === undefined ? undefined : "worker-slots-error"} onChange={(event) => setFields({ ...fields, computeSlots: event.currentTarget.value })} />{slotsError === undefined ? null : <small id="worker-slots-error" role="alert">{slotsError}</small>}</label>
        <label className="operation-check"><input type="checkbox" checked={fields.enabled} onChange={(event) => setFields({ ...fields, enabled: event.currentTarget.checked })} /><span>Enabled for scheduling</span></label>
        <footer><button type="button" className="secondary-button" onClick={props.onClose}>Dismiss</button><button type="submit" className="primary-button" disabled={props.submitting}>{props.submitting ? "Saving..." : "Save Worker"}</button></footer>
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
