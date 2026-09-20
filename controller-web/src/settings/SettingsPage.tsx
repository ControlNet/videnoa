import { CirclePause, CirclePlay, RotateCcw, Save } from "lucide-react"
import { useState } from "react"

import type { ApiClient, ApiClientError } from "../api/client"
import type { ServerSettings, SettingsUpdateRequest } from "../api/settingsSchemas"
import { Button } from "../ui/Button"
import { Status } from "../ui/Status"
import "../operations.css"
import { SettingsEditor, settingsFormId } from "./SettingsEditor"
import { useSettingsData } from "./useSettingsData"

type SettingsPageProps = { readonly apiClient: ApiClient }

type SettingsSaveReceipt = {
  readonly restartRequired: boolean
  readonly reconnectHref: string | null
}

type DegradedReconnect = {
  readonly actionError: ApiClientError
  readonly href: string
}

export function SettingsPage({ apiClient }: SettingsPageProps) {
  const data = useSettingsData(apiClient)
  const settings = data.settings
  const actionsEnabled = !data.mutating && !data.loading && data.error === null
  const [editorGeneration, setEditorGeneration] = useState(0)
  const [saveReceipt, setSaveReceipt] = useState<SettingsSaveReceipt | null>(null)
  const [degradedReconnect, setDegradedReconnect] = useState<DegradedReconnect | null>(null)

  async function save(request: SettingsUpdateRequest): Promise<boolean> {
    const previousSettings = data.settings
    if (previousSettings === null || !actionsEnabled) return false
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
    setEditorGeneration((generation) => generation + 1)
    const nextSettings = result.settings
    const endpointChanged = previousSettings.server.host !== nextSettings.server.host
      || previousSettings.server.port !== nextSettings.server.port
    setDegradedReconnect(null)
    setSaveReceipt({
      restartRequired: nextSettings.restart_required,
      reconnectHref: endpointChanged ? reconnectHref(nextSettings.server) : null,
    })
    return true
  }

  const degradedReconnectHref = data.actionError === degradedReconnect?.actionError ? degradedReconnect.href : null

  return (
    <div className="route-page operation-page settings-page">
      <div className="command-row">
        <h1>Settings</h1>
        <span className="spacer" />
        {settings === null ? null : <>
          {/* A pending restart is page state, not a Paths detail: it is what disables the control beside it. */}
          {!settings.restart_required ? null : (
            <span className="restart-pill"><RotateCcw size={12} aria-hidden="true" />Restart required</span>
          )}
          <span
            className={settings.scheduler.paused ? "scheduler-pill scheduler-pill--paused" : "scheduler-pill"}
            title={settings.restart_required ? "Scheduler control resumes once the Controller restarts on the saved paths." : undefined}
          >
            <Status tone={settings.scheduler.paused ? "quiet" : "positive"} label={settings.scheduler.paused ? "Scheduler paused" : "Scheduler running"} live={!settings.scheduler.paused} />
            <span className="scheduler-pill-divider" aria-hidden="true" />
            <Button
              size="sm"
              aria-label={settings.scheduler.paused ? "Resume scheduler" : "Pause scheduler"}
              disabled={!actionsEnabled || settings.restart_required}
              onClick={() => void data.setPaused(!settings.scheduler.paused)}
            >
              {settings.scheduler.paused ? <CirclePlay size={13} aria-hidden="true" /> : <CirclePause size={13} aria-hidden="true" />}
              {settings.scheduler.paused ? "Resume" : "Pause"}
            </Button>
          </span>
        </>}
      </div>
      {data.error === null ? null : <div className="operation-error alert alert--danger" role="alert"><span>{data.error}</span><button type="button" onClick={data.retry}>Retry</button></div>}
      {data.actionError === null ? null : <div className={degradedReconnectHref === null ? "operation-error alert alert--danger" : "operation-error alert alert--danger settings-degraded-error"} role="alert"><span>{settingsActionErrorMessage(data.actionError, data.loading, data.error)}{degradedReconnectHref === null ? null : " The Controller address changed and this page may disconnect."}</span>{degradedReconnectHref === null ? null : <a href={degradedReconnectHref}>Open Controller at the new address</a>}</div>}
      {saveReceipt === null ? null : <ConfigurationSaveReceipt receipt={saveReceipt} />}
      {settings === null ? <output className="operation-loading">{data.loading ? "Loading runtime settings..." : "Runtime settings are unavailable."}</output> : <>
        <SettingsEditor key={editorGeneration} settings={settings} actionError={data.actionError} onSave={save} />
        {/* Pinned as the route's last child: a long form never hides its own commit. */}
        <footer className="settings-save-bar">
          <span className="spacer" />
          <Button variant="primary" type="submit" form={settingsFormId} disabled={!actionsEnabled}>
            <Save size={13} aria-hidden="true" />
            {data.mutating ? "Saving and applying..." : "Save and apply settings"}
          </Button>
        </footer>
      </>}
    </div>
  )
}

function ConfigurationSaveReceipt({ receipt }: { readonly receipt: SettingsSaveReceipt }) {
  // Green is reserved for "this is live"; a save that still owes a restart is not.
  const tone = receipt.restartRequired ? "settings-save-receipt settings-save-receipt--pending" : "settings-save-receipt"
  return <output className={tone} aria-live="polite"><span className="settings-receipt-block"><strong>{receipt.restartRequired ? "Settings saved — restart required" : "Settings saved and applied"}</strong></span>{receipt.reconnectHref === null ? null : <span className="settings-receipt-block"><span>The Controller address changed and this page may disconnect.</span><a href={receipt.reconnectHref}>Open Controller at the new address</a></span>}</output>
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
