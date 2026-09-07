import { BrowserRouter } from "react-router"

import { AppErrorBoundary } from "./AppErrorBoundary"
import { BootstrapError } from "./auth/BootstrapError"
import { LoginPage } from "./auth/LoginPage"
import { SetupPage } from "./auth/SetupPage"
import { type AuthState, useSessionController } from "./auth/useSessionController"
import { AppShell } from "./shell/AppShell"

export function App() {
  return (
    <AppErrorBoundary>
      <BrowserRouter>
        <AuthGate />
      </BrowserRouter>
    </AppErrorBoundary>
  )
}

function AuthGate() {
  const controller = useSessionController()

  switch (controller.state.kind) {
    case "checking":
      return <LoadingSession />
    case "setup_required":
      return <SetupPage setup={controller.setup} />
    case "unauthenticated":
      return <LoginPage login={controller.login} notice={controller.state.notice} />
    case "authenticated":
      return <AppShell apiClient={controller.apiClient} logout={controller.logout} />
    case "bootstrap_error":
      return <BootstrapError message={controller.state.message} retry={controller.retryBootstrap} />
    default:
      return assertNever(controller.state)
  }
}

function LoadingSession() {
  return (
    <main className="login-page">
      <output className="session-loading">Checking Controller session...</output>
    </main>
  )
}

function assertNever(state: never): never {
  throw new UnknownAuthStateError(state)
}

class UnknownAuthStateError extends Error {
  readonly name = "UnknownAuthStateError"

  constructor(state: AuthState) {
    super("Unknown authentication state")
    void state
  }
}
