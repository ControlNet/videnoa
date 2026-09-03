import { ListTodo, LogOut, ServerCog, Settings } from "lucide-react"
import { useEffect, useLayoutEffect, useRef, useState } from "react"
import { Navigate, NavLink, Route, Routes, useLocation } from "react-router"

import type { ApiClient } from "../api/client"
import type { LogoutResult } from "../auth/useSessionController"
import { type ConnectionState, SessionEvents } from "../events/SessionEvents"
import { TasksPage } from "../tasks/TasksPage"
import { PlaceholderPage } from "./PlaceholderPage"

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

  useLayoutEffect(() => {
    const routeName = navigation.find(({ path }) => path === location.pathname)?.label ?? "Tasks"
    document.title = `${routeName} | Videnoa Controller`
    mainRef.current?.focus()
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

  useEffect(() => {
    if (logoutError !== null) logoutAlertRef.current?.focus()
  }, [logoutError])

  const connectionLabel = labelForConnection(connectionState)

  return (
    <div className="app-frame">
      <aside className="shell-sidebar">
        <div className="shell-brand">
          <span className="brand-mark" aria-hidden="true">V</span>
          <span>
            <strong>Videnoa</strong>
            <small>Controller</small>
          </span>
        </div>

        <nav className="primary-navigation" aria-label="Primary">
          {navigation.map(({ path, label, icon: Icon }) => (
            <NavLink key={path} to={path} className={({ isActive }) => isActive ? "nav-item active" : "nav-item"}>
              <Icon size={17} strokeWidth={1.75} aria-hidden="true" />
              <span>{label}</span>
            </NavLink>
          ))}
        </nav>

        <footer className="shell-footer">
          <output className="connection-status" aria-live="polite">
            <span className={`status-indicator ${connectionState}`} aria-hidden="true" />
            <span>{connectionLabel}</span>
            <code>/api/events</code>
          </output>
          <button
            aria-label={signingOut ? "Signing out..." : "Sign out"}
            className="signout-button"
            type="button"
            onClick={() => void handleLogout()}
            disabled={signingOut}
          >
            <LogOut size={16} strokeWidth={1.75} aria-hidden="true" />
            <span>{signingOut ? "Signing out..." : "Sign out"}</span>
          </button>
        </footer>
      </aside>

      <main className="shell-main" tabIndex={-1} ref={mainRef}>
        <Routes>
          <Route path="/" element={<Navigate to="/tasks" replace />} />
          <Route path="/tasks" element={<TasksPage apiClient={apiClient} />} />
          <Route path="/workers" element={<PlaceholderPage title="Workers" nextTask="TASK 18" description="Inspect processing capacity and the health of connected Videnoa nodes." />} />
          <Route path="/settings" element={<PlaceholderPage title="Settings" nextTask="TASK 18" description="Review scheduler policy, paths, retry behavior, and session boundaries." />} />
          <Route path="*" element={<Navigate to="/tasks" replace />} />
        </Routes>
      </main>
      {logoutError === null ? null : (
        <div className="shell-alert error-summary" role="alert" tabIndex={-1} ref={logoutAlertRef}>
          {logoutError}
        </div>
      )}
      <SessionEvents onConnectionStateChange={setConnectionState} />
    </div>
  )
}

function labelForConnection(state: ConnectionState): string {
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
