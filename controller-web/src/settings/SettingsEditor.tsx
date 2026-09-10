import { useEffect, useRef, useState } from "react"

import { CheckField, Field } from "../ui/Field"

import type { ApiClientError } from "../api/client"
import { type SettingsResponse, type SettingsUpdateRequest, settingsUpdateRequestSchema } from "../api/settingsSchemas"

type SettingsEditorProps = {
  readonly settings: SettingsResponse
  readonly actionError: ApiClientError | null
  readonly onSave: (request: SettingsUpdateRequest) => Promise<boolean>
}

type SettingsFields = {
  readonly dataRoot: string
  readonly cacheRoot: string
  readonly serverHost: string
  readonly serverPort: string
  readonly secureCookie: boolean
  readonly sessionAbsolute: string
  readonly sessionIdle: string
  readonly defaultSlots: string
  readonly prefetch: string
  readonly uploads: string
  readonly downloads: string
  readonly health: string
  readonly poll: string
  readonly transfer: string
  readonly retryInitial: string
  readonly retryMaximum: string
  readonly retryAttempts: string
}

const settingsFieldOrder = [
  "dataRoot",
  "cacheRoot",
  "serverHost",
  "serverPort",
  "sessionAbsolute",
  "sessionIdle",
  "defaultSlots",
  "prefetch",
  "uploads",
  "downloads",
  "health",
  "poll",
  "transfer",
  "retryInitial",
  "retryMaximum",
  "retryAttempts",
] as const satisfies readonly (keyof SettingsFields)[]
const settingsFieldNames = {
  dataRoot: "data_root", cacheRoot: "cache_root",
  serverHost: "host", serverPort: "port", secureCookie: "secure_cookie", sessionAbsolute: "session_absolute_seconds", sessionIdle: "session_idle_seconds",
  defaultSlots: "default_compute_slots", prefetch: "prefetch_per_worker", uploads: "max_concurrent_uploads", downloads: "max_concurrent_downloads",
  health: "health_seconds", poll: "poll_seconds", transfer: "transfer_seconds", retryInitial: "initial_seconds", retryMaximum: "maximum_seconds", retryAttempts: "max_attempts",
} as const satisfies Record<keyof SettingsFields, string>

export const settingsFormId = "settings-editor-form"

export function SettingsEditor({ settings, actionError, onSave }: SettingsEditorProps) {
  const formRef = useRef<HTMLFormElement>(null)
  const [baseline, setBaseline] = useState(settings)
  const [remoteChange, setRemoteChange] = useState(false)
  const [fields, setFields] = useState<SettingsFields>(() => fieldsFrom(settings))
  const [fieldErrors, setFieldErrors] = useState<Partial<Record<keyof SettingsFields, string>>>({})

  // Reconcile only authoritative versions, not the versionless optimistic scheduler delta.
  // Preserve dirty fields and refresh untouched ones without remounting focused inputs.
  if (settings.version !== baseline.version) {
    const previous = fieldsFrom(baseline)
    const incoming = fieldsFrom(settings)
    const dirty = (Object.keys(fields) as (keyof SettingsFields)[]).filter((key) => fields[key] !== previous[key])
    setFields({ ...incoming, ...Object.fromEntries(dirty.map((key) => [key, fields[key]])) })
    setBaseline(settings)
    setRemoteChange(remoteChange || dirty.length > 0)
  }

  function focusFirstError(errors: Partial<Record<keyof SettingsFields, string>>): void {
    const firstInvalidField = settingsFieldOrder.find((field) => errors[field] !== undefined)
    if (firstInvalidField === undefined) return
    const input = formRef.current?.elements.namedItem(settingsFieldNames[firstInvalidField])
    if (input instanceof HTMLInputElement) input.focus()
  }

  useEffect(() => {
    const errors = serverFieldErrors(actionError)
    const firstInvalidField = settingsFieldOrder.find((field) => errors[field] !== undefined)
    if (firstInvalidField === undefined) return
    const input = formRef.current?.elements.namedItem(settingsFieldNames[firstInvalidField])
    if (input instanceof HTMLInputElement) input.focus()
  }, [actionError])

  async function submit(): Promise<void> {
    const parsed = settingsUpdateRequestSchema.safeParse({
      version: settings.version,
      paths: {
        data_root: fields.dataRoot.trim(),
        cache_root: fields.cacheRoot.trim(),
      },
      server: {
        host: fields.serverHost.trim(),
        port: Number(fields.serverPort),
      },
      auth: {
        secure_cookie: fields.secureCookie,
        session_absolute_seconds: Number(fields.sessionAbsolute),
        session_idle_seconds: Number(fields.sessionIdle),
      },
      scheduler: {
        paused: settings.scheduler.paused,
        default_compute_slots: Number(fields.defaultSlots),
        prefetch_per_worker: Number(fields.prefetch),
        max_concurrent_uploads: Number(fields.uploads),
        max_concurrent_downloads: Number(fields.downloads),
      },
      timeouts: {
        health_seconds: Number(fields.health),
        poll_seconds: Number(fields.poll),
        transfer_seconds: Number(fields.transfer),
      },
      retry: {
        initial_seconds: Number(fields.retryInitial),
        maximum_seconds: Number(fields.retryMaximum),
        max_attempts: Number(fields.retryAttempts),
      },
    })
    if (!parsed.success) {
      const errors: Partial<Record<keyof SettingsFields, string>> = {}
      for (const issue of parsed.error.issues) {
        const field = fieldForPath(issue.path.map(String))
        if (field !== null) errors[field] = issue.message
      }
      setFieldErrors(errors)
      focusFirstError(errors)
      return
    }
    setFieldErrors({})
    await onSave(parsed.data)
  }

  const serverErrors = serverFieldErrors(actionError)
  return (
    <form ref={formRef} id={settingsFormId} className="settings-editor" noValidate onSubmit={(event) => { event.preventDefault(); void submit() }}>
      <nav className="settings-nav" aria-label="Settings sections">
        {sectionIndex.map(({ id, title }) => <a key={id} href={`#${id}`}>{title}</a>)}
      </nav>

      <div className="settings-sections">
        {remoteChange && <p className="alert" role="status">Settings changed on the Controller. Your unsaved edits are preserved; other fields reflect the latest settings. Review before saving: your edited values will replace the current values on the Controller.</p>}
        <SettingsSection id="settings-paths" title="Paths">
          <TextField label="Data root" name="data_root" value={fields.dataRoot} error={fieldErrors.dataRoot ?? serverErrors.dataRoot} hint="Stores controller.toml, the task database, authentication state, and Controller identity." onChange={(dataRoot) => setFields({ ...fields, dataRoot })} />
          <TextField label="Cache root" name="cache_root" value={fields.cacheRoot} error={fieldErrors.cacheRoot ?? serverErrors.cacheRoot} hint="Stores downloaded output before publication. Use a dedicated directory on the output filesystem." onChange={(cacheRoot) => setFields({ ...fields, cacheRoot })} />
          <p className="settings-path-note settings-span-2">
            {settings.restart_required
              ? "Restart the Controller to activate these paths. The scheduler remains paused until restart."
              : "Path changes require a Controller restart. Pause the scheduler and wait for active tasks to finish before saving."}
          </p>
        </SettingsSection>
        <SettingsSection id="settings-server" title="Server binding">
          <TextField label="Server host" name="host" value={fields.serverHost} error={fieldErrors.serverHost ?? serverErrors.serverHost} onChange={(serverHost) => setFields({ ...fields, serverHost })} />
          <NumberField label="Server port" name="port" value={fields.serverPort} min={1} max={65_535} error={fieldErrors.serverPort ?? serverErrors.serverPort} onChange={(serverPort) => setFields({ ...fields, serverPort })} />
        </SettingsSection>
        <SettingsSection id="settings-auth" title="Authentication policy">
          <NumberField label="Absolute session seconds" name="session_absolute_seconds" value={fields.sessionAbsolute} min={1} error={fieldErrors.sessionAbsolute ?? serverErrors.sessionAbsolute} onChange={(sessionAbsolute) => setFields({ ...fields, sessionAbsolute })} />
          <NumberField label="Idle session seconds" name="session_idle_seconds" value={fields.sessionIdle} min={1} error={fieldErrors.sessionIdle ?? serverErrors.sessionIdle} onChange={(sessionIdle) => setFields({ ...fields, sessionIdle })} />
          <div className="settings-span-2">
            <CheckField id="settings-secure_cookie" name="secure_cookie" label="Require secure session cookie" checked={fields.secureCookie} onChange={(secureCookie) => setFields({ ...fields, secureCookie })} />
          </div>
        </SettingsSection>
        <SettingsSection id="settings-scheduler" title="Scheduler capacity">
          <NumberField label="Default compute slots" name="default_compute_slots" value={fields.defaultSlots} min={1} max={65_535} error={fieldErrors.defaultSlots ?? serverErrors.defaultSlots} onChange={(defaultSlots) => setFields({ ...fields, defaultSlots })} />
          <NumberField label="Prefetch per worker" name="prefetch_per_worker" value={fields.prefetch} min={0} max={65_535} error={fieldErrors.prefetch ?? serverErrors.prefetch} onChange={(prefetch) => setFields({ ...fields, prefetch })} />
          <NumberField label="Concurrent uploads" name="max_concurrent_uploads" value={fields.uploads} min={1} max={65_535} error={fieldErrors.uploads ?? serverErrors.uploads} onChange={(uploads) => setFields({ ...fields, uploads })} />
          <NumberField label="Concurrent downloads" name="max_concurrent_downloads" value={fields.downloads} min={1} max={65_535} error={fieldErrors.downloads ?? serverErrors.downloads} onChange={(downloads) => setFields({ ...fields, downloads })} />
        </SettingsSection>
        <SettingsSection id="settings-timeouts" title="Timeouts">
          <NumberField label="Health timeout seconds" name="health_seconds" value={fields.health} min={1} max={604_800} error={fieldErrors.health ?? serverErrors.health} onChange={(health) => setFields({ ...fields, health })} />
          <NumberField label="Poll timeout seconds" name="poll_seconds" value={fields.poll} min={1} max={604_800} error={fieldErrors.poll ?? serverErrors.poll} onChange={(poll) => setFields({ ...fields, poll })} />
          <NumberField label="Transfer timeout seconds" name="transfer_seconds" value={fields.transfer} min={1} max={604_800} error={fieldErrors.transfer ?? serverErrors.transfer} onChange={(transfer) => setFields({ ...fields, transfer })} />
        </SettingsSection>
        <SettingsSection id="settings-retry" title="Retry policy">
          <NumberField label="Initial retry seconds" name="initial_seconds" value={fields.retryInitial} min={1} max={604_800} error={fieldErrors.retryInitial ?? serverErrors.retryInitial} onChange={(retryInitial) => setFields({ ...fields, retryInitial })} />
          <NumberField label="Maximum retry seconds" name="maximum_seconds" value={fields.retryMaximum} min={1} max={604_800} error={fieldErrors.retryMaximum ?? serverErrors.retryMaximum} onChange={(retryMaximum) => setFields({ ...fields, retryMaximum })} />
          <NumberField label="Maximum retry attempts" name="max_attempts" value={fields.retryAttempts} min={1} max={100} error={fieldErrors.retryAttempts ?? serverErrors.retryAttempts} onChange={(retryAttempts) => setFields({ ...fields, retryAttempts })} />
        </SettingsSection>
      </div>

    </form>
  )
}

const sectionIndex = [
  { id: "settings-paths", title: "Paths" },
  { id: "settings-server", title: "Server binding" },
  { id: "settings-auth", title: "Authentication" },
  { id: "settings-scheduler", title: "Scheduler" },
  { id: "settings-timeouts", title: "Timeouts" },
  { id: "settings-retry", title: "Retry policy" },
] as const

type NumberFieldProps = { readonly label: string; readonly name: string; readonly value: string; readonly min: number; readonly max?: number; readonly error: string | undefined; readonly onChange: (value: string) => void }

type TextFieldProps = { readonly label: string; readonly name: string; readonly value: string; readonly error: string | undefined; readonly hint?: React.ReactNode; readonly onChange: (value: string) => void }

function TextField(props: TextFieldProps) {
  return (
    <Field
      id={`settings-${props.name}`}
      label={props.label}
      name={props.name}
      type="text"
      spellCheck={false}
      value={props.value}
      error={props.error}
      hint={props.hint}
      onChange={(event) => props.onChange(event.currentTarget.value)}
    />
  )
}

function NumberField(props: NumberFieldProps) {
  return (
    <Field
      id={`settings-${props.name}`}
      label={props.label}
      name={props.name}
      type="number"
      inputMode="numeric"
      min={props.min}
      max={props.max}
      value={props.value}
      error={props.error}
      onChange={(event) => props.onChange(event.currentTarget.value)}
    />
  )
}

function SettingsSection({ id, title, children }: { readonly id: string; readonly title: string; readonly children: React.ReactNode }) {
  return (
    <section id={id} className="operation-section" aria-label={title}>
      <header className="section-head"><h2>{title}</h2></header>
      <div className="settings-grid">{children}</div>
    </section>
  )
}

function fieldsFrom(settings: SettingsResponse): SettingsFields {
  return {
    dataRoot: settings.paths.data_root, cacheRoot: settings.paths.cache_root,
    serverHost: settings.server.host, serverPort: String(settings.server.port), secureCookie: settings.secure_cookie,
    sessionAbsolute: String(settings.session_absolute_seconds), sessionIdle: String(settings.session_idle_seconds),
    defaultSlots: String(settings.scheduler.default_compute_slots), prefetch: String(settings.scheduler.prefetch_per_worker),
    uploads: String(settings.scheduler.max_concurrent_uploads), downloads: String(settings.scheduler.max_concurrent_downloads),
    health: String(settings.timeouts.health_seconds), poll: String(settings.timeouts.poll_seconds), transfer: String(settings.timeouts.transfer_seconds),
    retryInitial: String(settings.retry.initial_seconds), retryMaximum: String(settings.retry.maximum_seconds), retryAttempts: String(settings.retry.max_attempts),
  }
}

function fieldForPath(path: readonly string[]): keyof SettingsFields | null {
  const field = path.at(-1)
  if (field === "data_root" || field === "paths") return "dataRoot"
  if (field === "cache_root") return "cacheRoot"
  if (field === "host" || field === "server") return "serverHost"
  if (field === "port") return "serverPort"
  if (field === "session_absolute_seconds") return "sessionAbsolute"
  if (field === "session_idle_seconds" || field === "auth") return "sessionIdle"
  if (field === "default_compute_slots") return "defaultSlots"
  if (field === "prefetch_per_worker") return "prefetch"
  if (field === "max_concurrent_uploads") return "uploads"
  if (field === "max_concurrent_downloads") return "downloads"
  if (field === "health_seconds") return "health"
  if (field === "poll_seconds") return "poll"
  if (field === "transfer_seconds") return "transfer"
  if (field === "initial_seconds") return "retryInitial"
  if (field === "maximum_seconds") return "retryMaximum"
  if (field === "max_attempts") return "retryAttempts"
  return null
}

function serverFieldErrors(error: ApiClientError | null): Partial<Record<keyof SettingsFields, string>> {
  const result: Partial<Record<keyof SettingsFields, string>> = {}
  for (const fieldError of error?.fieldErrors ?? []) {
    const field = fieldForPath(fieldError.field.split("."))
    if (field !== null) result[field] = fieldError.message
    if (fieldError.field === "retry") result.retryInitial = fieldError.message
  }
  return result
}
