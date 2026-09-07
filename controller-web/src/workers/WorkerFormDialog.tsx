import { Eye, EyeOff, X } from "lucide-react"
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
import { CheckField, Field, SegmentedField } from "../ui/Field"
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
  readonly transport: "http" | "iroh"
  readonly apiUrl: string
  readonly computeSlots: string
  readonly password: string
  readonly clearPassword: boolean
  readonly enabled: boolean
}

const emptyFields: WorkerFields = { transport: "http", name: "", apiUrl: "", computeSlots: "1", enabled: true, password: "", clearPassword: false }

export function WorkerFormDialog(props: WorkerFormDialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null)
  const firstInputRef = useRef<HTMLInputElement>(null)
  const apiUrlRef = useRef<HTMLInputElement>(null)
  const computeSlotsRef = useRef<HTMLInputElement>(null)
  const [fields, setFields] = useState<WorkerFields>(() => props.worker === null ? emptyFields : {
    password: "",
    clearPassword: false,
    name: props.worker.name,
    transport: props.worker.transport ?? "http",
    apiUrl: props.worker.endpoint_id ?? props.worker.api_url ?? "",
    computeSlots: String(props.worker.compute_slots),
    enabled: props.worker.enabled,
  })
  const [showPassword, setShowPassword] = useState(false)
  const passwordRef = useRef<HTMLInputElement>(null)
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
      ...(fields.transport === "iroh" ? { transport: "iroh" as const, endpoint_id: fields.apiUrl.trim() } : { api_url: fields.apiUrl }),
      enabled: fields.enabled,
      compute_slots: Number(fields.computeSlots),
    }
    const worker = props.worker
    if (worker === null) {
      const parsed = workerCreateRequestSchema.safeParse({ ...raw, ...(fields.password === "" ? {} : { password: fields.password }) })
      if (!parsed.success) {
        const errors = workerFieldErrors(parsed.error.issues)
        setFieldErrors(errors)
        if (errors.name !== undefined) firstInputRef.current?.focus()
        else if (errors.apiUrl !== undefined) apiUrlRef.current?.focus()
        else if (errors.computeSlots !== undefined) computeSlotsRef.current?.focus()
        else if (errors.password !== undefined) passwordRef.current?.focus()
        return
      }
      if (await props.onCreate(parsed.data)) props.onClose()
      return
    }
    const parsed = workerUpdateRequestSchema.safeParse({ ...raw, version: worker.version, ...(fields.clearPassword ? { password: null } : fields.password === "" ? {} : { password: fields.password }) })
    if (!parsed.success) {
      const errors = workerFieldErrors(parsed.error.issues)
      setFieldErrors(errors)
      if (errors.name !== undefined) firstInputRef.current?.focus()
      else if (errors.apiUrl !== undefined) apiUrlRef.current?.focus()
      else if (errors.computeSlots !== undefined) computeSlotsRef.current?.focus()
        else if (errors.password !== undefined) passwordRef.current?.focus()
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
        else if (errors.password !== undefined) passwordRef.current?.focus()
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
        <SegmentedField
          name="transport"
          label="Connection type"
          value={fields.transport}
          options={[{ value: "http", label: "HTTP / HTTPS" }, { value: "iroh", label: "Iroh" }]}
          onChange={(transport) => setFields((current) => ({ ...current, transport, apiUrl: "", clearPassword: false }))}
        />
        <Field ref={apiUrlRef} id="worker-api-url" label={fields.transport === "iroh" ? "Worker Endpoint ID" : "Worker API URL"} name={fields.transport === "iroh" ? "endpoint_id" : "api_url"} type={fields.transport === "iroh" ? "text" : "url"} autoComplete="off" spellCheck={false} value={fields.apiUrl} error={urlError} onChange={(event) => setFields({ ...fields, apiUrl: event.currentTarget.value })} />
        <div className="worker-password-field">
          <Field ref={passwordRef} id="worker-password" label={fields.transport === "iroh" ? "Access password" : "Access password (optional)"} name="password" type={showPassword ? "text" : "password"} autoComplete="new-password" spellCheck={false} value={fields.password} disabled={fields.clearPassword} error={fieldErrors.password ?? serverFields.password} onChange={(event) => setFields({ ...fields, password: event.currentTarget.value })} />
          <Button variant="outline" size="sm" aria-label={showPassword ? "Hide password" : "Show password"} aria-pressed={showPassword} onClick={() => setShowPassword(!showPassword)}>{showPassword ? <EyeOff size={14} aria-hidden="true" /> : <Eye size={14} aria-hidden="true" />}{showPassword ? "Hide" : "Show"}</Button>
          <p className="text-muted">{props.worker?.has_password ? "A password is saved. Leave blank to keep it, enter a new password to replace it, or clear it below." : fields.transport === "iroh" ? "Enter the password configured on this worker." : "Leave blank if this worker does not require a password."}</p>
          {props.worker?.has_password && fields.transport !== "iroh" && <CheckField id="worker-clear-password" name="clear_password" label="Clear saved password" checked={fields.clearPassword} onChange={(clearPassword) => setFields({ ...fields, clearPassword, password: clearPassword ? "" : fields.password })} />}
        </div>
        <Field ref={computeSlotsRef} id="worker-slots" label="Compute slots" name="compute_slots" type="number" inputMode="numeric" min={1} max={65535} value={fields.computeSlots} error={slotsError} onChange={(event) => setFields({ ...fields, computeSlots: event.currentTarget.value })} />
        <CheckField id="worker-enabled" name="enabled" label="Enabled for scheduling" checked={fields.enabled} onChange={(enabled) => setFields({ ...fields, enabled })} />
        <footer>
          <span className="spacer" />
          <Button variant="primary" type="submit" disabled={props.submitting}>{props.submitting ? "Saving..." : "Save Worker"}</Button>
        </footer>
      </form>
    </dialog>
  )
}

function workerFieldErrors(issues: readonly { readonly path: readonly PropertyKey[]; readonly message: string }[]): Partial<Record<keyof WorkerFields, string>> {
  const errors: Partial<Record<keyof WorkerFields, string>> = {}
  for (const issue of issues) {
    if (issue.path[0] === "password") errors.password = issue.message
    if (issue.path[0] === "name") errors.name = issue.message
    if (issue.path[0] === "api_url" || issue.path[0] === "endpoint_id") errors.apiUrl = issue.message
    if (issue.path[0] === "compute_slots") errors.computeSlots = "Enter 1 to 65535 compute slots."
  }
  return errors
}
