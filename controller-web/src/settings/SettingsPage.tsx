import { CirclePause, CirclePlay } from "lucide-react"
import { useState } from "react"

import type { ApiClient, ApiClientError } from "../api/client"
import type { Readiness, ServerSettings, SettingsResponse, SettingsUpdateRequest } from "../api/settingsSchemas"
import "../operations.css"
import { SettingsEditor } from "./SettingsEditor"
import { useSettingsData } from "./useSettingsData"

type SettingsPageProps = { readonly apiClient: ApiClient }

type SettingsSaveReceipt = {
  readonly configFile: string
  readonly reconnectHref: string | null
}

type DegradedReconnect = {
  readonly actionError: ApiClientError
  readonly href: string
}

export function SettingsPage({ apiClient }: SettingsPageProps) {
  const data = useSettingsData(apiClient)
  const settings = data.settings
  const actionsEnabled = !data.mutating && !data.loading
  const [saveReceipt, setSaveReceipt] = useState<SettingsSaveReceipt | null>(null)
  const [degradedReconnect, setDegradedReconnect] = useState<DegradedReconnect | null>(null)

  async function save(request: SettingsUpdateRequest): Promise<boolean> {
    const previousSettings = data.settings
    if (previousSettings === null) return false
    const result = await data.save(request)
    if (!result.ok) {
      const endpointChanged = previousSettings.server.host !== request.server.host
        || previousSettings.server.port !== request.server.port
      setSaveReceipt(null)
      setDegradedReconnect(result.error.code === "unavailable" && result.error.retryable && endpointChanged
        ? { actionError: result.error, href: reconnectHref(request.server) }
        : null)
      return false
    }
    const nextSettings = result.settings
    const endpointChanged = previousSettings.server.host !== nextSettings.server.host
      || previousSettings.server.port !== nextSettings.server.port
    setDegradedReconnect(null)
    setSaveReceipt({
      configFile: nextSettings.paths.config_file,
      reconnectHref: endpointChanged ? reconnectHref(nextSettings.server) : null,
    })
    return true
  }

  const degradedReconnectHref = data.actionError === degradedReconnect?.actionError ? degradedReconnect.href : null

  return (
    <div className="route-page operation-page settings-page">
      <header className="operation-header"><div><p className="technical-label">CONTROLLER POLICY</p><h1>Settings</h1><p>Adjust server, session, scheduler, timeout, and retry policy through one durable configuration boundary.</p></div>{settings === null ? null : <button type="button" className={settings.scheduler.paused ? "primary-button compact-action" : "secondary-button compact-action"} aria-label={settings.scheduler.paused ? "Resume scheduler" : "Pause scheduler"} disabled={!actionsEnabled} onClick={() => void data.setPaused(!settings.scheduler.paused)}>{settings.scheduler.paused ? <CirclePlay size={16} aria-hidden="true" /> : <CirclePause size={16} aria-hidden="true" />}{settings.scheduler.paused ? "Resume" : "Pause"}</button>}</header>
      {data.error === null ? null : <div className="operation-error" role="alert"><span>{data.error}</span><button type="button" onClick={data.retry}>Retry</button></div>}
      {data.actionError === null ? null : <div className={degradedReconnectHref === null ? "operation-error" : "operation-error settings-degraded-error"} role="alert"><span>{settingsActionErrorMessage(data.actionError, data.loading, data.error)}{degradedReconnectHref === null ? null : " The Controller address changed and this page may disconnect."}</span>{degradedReconnectHref === null ? null : <a href={degradedReconnectHref}>Open Controller at the new address</a>}</div>}
      {saveReceipt === null ? null : <ConfigurationSaveReceipt receipt={saveReceipt} />}
      {settings === null ? <output className="operation-loading">{data.loading ? "Loading runtime settings..." : "Runtime settings are unavailable."}</output> : <>
        <section className="scheduler-state" aria-label="Scheduler state"><div><span className={`operation-status ${settings.scheduler.paused ? "offline" : "healthy"}`}>{settings.scheduler.paused ? "Paused" : "Running"}</span><strong>{settings.scheduler.paused ? "New starts held" : "New work admitted"}</strong></div><p>Pause blocks new reservations, prefetch, and compute starts. Already-running processing continues; transfer and publication continue where applicable; cleanup continues.</p></section>
        <SettingsEditor key={settings.version} settings={settings} actionError={data.actionError} actionsEnabled={actionsEnabled} saving={data.mutating} onSave={save} />
        <ReadOnlyConfiguration settings={settings} readiness={data.readiness} />
      </>}
    </div>
  )
}

function ConfigurationSaveReceipt({ receipt }: { readonly receipt: SettingsSaveReceipt }) {
  return <output className="settings-save-receipt" aria-live="polite"><span className="settings-receipt-block"><strong>Settings saved and applied</strong><span>Configuration file {receipt.configFile} was written and the returned settings are active.</span></span>{receipt.reconnectHref === null ? null : <span className="settings-receipt-block"><span>The Controller address changed and this page may disconnect.</span><a href={receipt.reconnectHref}>Open Controller at the new address</a></span>}</output>
}

function ReadOnlyConfiguration({ settings, readiness }: { readonly settings: SettingsResponse; readonly readiness: Readiness | null }) {
  const rows = [
    ["Workspace", settings.paths.workspace],
    ["Data root", settings.paths.data_root],
    ["Configuration file", settings.paths.config_file],
  ] as const
  return <section className="operation-section read-only-settings"><header><div><h2>Controller paths</h2><p>Safe runtime locations reported by the Controller for operational context.</p></div><span className={`operation-status ${readiness?.status === "ready" ? "healthy" : "offline"}`}>{readiness?.status === "ready" ? "Ready" : "Not ready"}</span></header><dl>{rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd title={value}>{value}</dd></div>)}</dl>{readiness?.checks.map((check) => <p className="readiness-check" key={check.name}><strong>{check.name}</strong><span>{check.ready ? "Ready" : check.message ?? "Not ready"}</span></p>)}</section>
}

function reconnectHref(server: ServerSettings): string {
  const host = server.host === "0.0.0.0" || server.host === "::" ? window.location.hostname : server.host
  const authorityHost = host.includes(":") ? `[${host}]` : host
  return `${window.location.protocol}//${authorityHost}:${server.port}/`
}

function settingsActionErrorMessage(error: ApiClientError, loading: boolean, loadError: string | null): string {
  if (error.code !== "conflict") return error.message
  if (loading) return "Settings changed on the Controller. Reloading current values before another update can be submitted."
  if (loadError !== null) return "Settings changed on the Controller, but current values could not be reloaded."
  return "Settings changed on the Controller. Current values were reloaded; review them before retrying."
}
