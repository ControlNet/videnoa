import { CirclePause, CirclePlay } from "lucide-react"

import type { ApiClient } from "../api/client"
import "../operations.css"
import { SettingsEditor } from "./SettingsEditor"
import { useSettingsData } from "./useSettingsData"

type SettingsPageProps = { readonly apiClient: ApiClient }

export function SettingsPage({ apiClient }: SettingsPageProps) {
  const data = useSettingsData(apiClient)
  const settings = data.settings
  return (
    <div className="route-page operation-page settings-page">
      <header className="operation-header"><div><p className="technical-label">RUNTIME POLICY</p><h1>Settings</h1><p>Adjust live scheduler behavior while keeping restart-required configuration read-only.</p></div>{settings === null ? null : <button type="button" className={settings.scheduler.paused ? "primary-button compact-action" : "secondary-button compact-action"} aria-label={settings.scheduler.paused ? "Resume scheduler" : "Pause scheduler"} disabled={data.mutating} onClick={() => void data.setPaused(!settings.scheduler.paused)}>{settings.scheduler.paused ? <CirclePlay size={16} aria-hidden="true" /> : <CirclePause size={16} aria-hidden="true" />}{settings.scheduler.paused ? "Resume" : "Pause"}</button>}</header>
      {data.error === null ? null : <div className="operation-error" role="alert"><span>{data.error}</span><button type="button" onClick={data.retry}>Retry</button></div>}
      {data.actionError === null ? null : <div className="operation-error" role="alert">{data.actionError.code === "conflict" ? "Settings changed on the Controller. Current values were reloaded; review them before retrying." : data.actionError.message}</div>}
      {settings === null ? <output className="operation-loading">{data.loading ? "Loading runtime settings..." : "Runtime settings are unavailable."}</output> : <>
        <section className="scheduler-state" aria-label="Scheduler state"><div><span className={`operation-status ${settings.scheduler.paused ? "offline" : "healthy"}`}>{settings.scheduler.paused ? "Paused" : "Running"}</span><strong>{settings.scheduler.paused ? "New starts held" : "New work admitted"}</strong></div><p>Pause blocks new reservations, prefetch, and compute starts. Already-running processing continues; transfer and publication continue where applicable; cleanup continues.</p></section>
        <SettingsEditor key={settings.version} settings={settings} actionError={data.actionError} saving={data.mutating} onSave={data.save} />
        <ReadOnlyConfiguration settings={settings} readiness={data.readiness} />
      </>}
    </div>
  )
}

function ReadOnlyConfiguration({ settings, readiness }: { readonly settings: NonNullable<ReturnType<typeof useSettingsData>["settings"]>; readonly readiness: ReturnType<typeof useSettingsData>["readiness"] }) {
  const rows = [
    ["Input roots", settings.paths.input_roots.join(" · ")], ["Output roots", settings.paths.output_roots.join(" · ")],
    ["Data root", settings.paths.data_root], ["Temporary root", settings.paths.temp_root], ["Password hash file", settings.paths.password_hash_file],
    ["Secure cookie", settings.secure_cookie ? "Required" : "Not required"], ["Absolute session", `${settings.session_absolute_seconds} seconds`], ["Idle session", `${settings.session_idle_seconds} seconds`],
  ] as const
  return <section className="operation-section read-only-settings"><header><div><h2>Restart-required configuration</h2><p>Change these values in Controller configuration, then restart the service. Secret contents are never exposed.</p></div><span className={`operation-status ${readiness?.status === "ready" ? "healthy" : "offline"}`}>{readiness?.status === "ready" ? "Ready" : "Not ready"}</span></header><dl>{rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd title={value}>{value || "None configured"}</dd></div>)}</dl>{readiness?.checks.map((check) => <p className="readiness-check" key={check.name}><strong>{check.name}</strong><span>{check.ready ? "Ready" : check.message ?? "Not ready"}</span></p>)}</section>
}
