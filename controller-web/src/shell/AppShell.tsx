import { ListTodo, LogOut, ServerCog, Settings } from "lucide-react"
import { useLayoutEffect, useRef, useState } from "react"
import { Navigate, NavLink, Route, Routes, useLocation } from "react-router"

import type { ApiClient } from "../api/client"
import type { LogoutResult } from "../auth/useSessionController"
import { type ConnectionState, SessionEvents } from "../events/SessionEvents"
import { SettingsPage } from "../settings/SettingsPage"
import { TasksPage } from "../tasks/TasksPage"
import { Button } from "../ui/Button"
import { Status, type Tone } from "../ui/Status"
import { useMediaQuery } from "../ui/useMediaQuery"
import { WorkersPage } from "../workers/WorkersPage"
import { BuildInfo } from "./BuildInfo"
import "./shell.css"

type AppShellProps = {
  readonly apiClient: ApiClient
  readonly logout: () => Promise<LogoutResult>
}

const navigation = [
  { path: "/tasks", label: "Tasks", icon: ListTodo },
  { path: "/workers", label: "Workers", icon: ServerCog },
  { path: "/settings", label: "Settings", icon: Settings },
] as const

export function AppShell({ apiClient, logout }: AppShellProps) {
  const location = useLocation()
  const mainRef = useRef<HTMLElement>(null)
  const logoutAlertRef = useRef<HTMLDivElement>(null)
  const [signingOut, setSigningOut] = useState(false)
  const [logoutError, setLogoutError] = useState<string | null>(null)
  const [connectionState, setConnectionState] = useState<ConnectionState>("connecting")
  const narrow = useMediaQuery("(max-width: 48rem)")

  useLayoutEffect(() => {
    const routeName = navigation.find(({ path }) => path === location.pathname)?.label ?? "Tasks"
    document.title = `${routeName} | Videnoa Controller`
    const focusTarget = logoutAlertRef.current ?? mainRef.current
    focusTarget?.focus()
  }, [location.pathname])

  async function handleLogout() {
    setSigningOut(true)
    setLogoutError(null)
    try {
      const result = await logout()
      if (!result.ok) setLogoutError(result.message)
    } finally {
      setSigningOut(false)
    }
  }

  useLayoutEffect(() => {
    if (logoutError !== null) logoutAlertRef.current?.focus()
  }, [logoutError])

  const signOutLabel = signingOut ? "Signing out..." : "Sign out"

  return (
    <div className="app-frame">
      <aside className="shell-sidebar">
        <div className="shell-brand">
          <span className="brand-mark" aria-hidden="true">V</span>
          <span>
            <strong>Videnoa</strong>
            <small>CONTROLLER</small>
          </span>
        </div>

        {narrow ? null : (
          <nav className="primary-navigation" aria-label="Primary">
            {navigation.map(({ path, label, icon: Icon }) => (
              <NavLink key={path} to={path} className={({ isActive }) => (isActive ? "nav-item active" : "nav-item")}>
                <Icon size={15} strokeWidth={1.8} aria-hidden="true" />
                <span>{label}</span>
              </NavLink>
            ))}
          </nav>
        )}

        <div className="shell-footer">
          {/* Stays mounted so a degraded stream is announced, but shows nothing while healthy. */}
          <output className="connection-status" aria-live="polite">
            {connectionState === "connected" || connectionState === "connecting" ? null : (
              <Status tone={connectionTone(connectionState)} label={connectionLabel(connectionState)} />
            )}
          </output>
          {narrow ? null : (
            <Button aria-label={signOutLabel} onClick={() => void handleLogout()} disabled={signingOut}>
              <LogOut size={14} strokeWidth={1.8} aria-hidden="true" />
              <span>{signOutLabel}</span>
            </Button>
          )}
          <BuildInfo apiClient={apiClient} />
        </div>
      </aside>

      {logoutError === null ? null : (
        <div className="shell-alert alert alert--danger" role="alert" tabIndex={-1} ref={logoutAlertRef}>
          {logoutError}
        </div>
      )}

      <main className="shell-main" tabIndex={-1} ref={mainRef}>
        <Routes>
          <Route path="/" element={<Navigate to="/tasks" replace />} />
          <Route path="/tasks" element={<TasksPage apiClient={apiClient} />} />
          <Route path="/workers" element={<WorkersPage apiClient={apiClient} />} />
          <Route path="/settings" element={<SettingsPage apiClient={apiClient} />} />
          <Route path="*" element={<Navigate to="/tasks" replace />} />
        </Routes>
      </main>

      {!narrow ? null : (
        <nav className="shell-tabs" aria-label="Primary">
          {navigation.map(({ path, label, icon: Icon }) => (
            <NavLink key={path} to={path} className={({ isActive }) => (isActive ? "active" : undefined)}>
              <Icon size={18} strokeWidth={1.8} aria-hidden="true" />
              <span>{label}</span>
            </NavLink>
          ))}
          <button type="button" aria-label={signOutLabel} disabled={signingOut} onClick={() => void handleLogout()}>
            <LogOut size={18} strokeWidth={1.8} aria-hidden="true" />
            <span>{signOutLabel}</span>
          </button>
        </nav>
      )}

      <SessionEvents onConnectionStateChange={setConnectionState} />
    </div>
  )
}

function connectionLabel(state: ConnectionState): string {
  switch (state) {
    case "connecting":
      return "Controller connecting"
    case "connected":
      return "Controller connected"
    case "reconnecting":
      return "Controller reconnecting"
    case "unavailable":
      return "Controller unavailable"
  }
}

function connectionTone(state: ConnectionState): Tone {
  switch (state) {
    case "connecting":
      return "quiet"
    case "connected":
      return "positive"
    case "reconnecting":
      return "active"
    case "unavailable":
      return "negative"
  }
}
