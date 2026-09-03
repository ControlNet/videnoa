import { Component, createRef, type ReactNode } from "react"

type AppErrorBoundaryProps = {
  readonly children: ReactNode
}

type AppErrorBoundaryState = {
  readonly interrupted: boolean
}

export class AppErrorBoundary extends Component<AppErrorBoundaryProps, AppErrorBoundaryState> {
  readonly state: AppErrorBoundaryState = { interrupted: false }
  private readonly retryRef = createRef<HTMLButtonElement>()

  static getDerivedStateFromError(): AppErrorBoundaryState {
    return { interrupted: true }
  }

  componentDidCatch(): void {
    this.retryRef.current?.focus()
  }

  render(): ReactNode {
    if (!this.state.interrupted) return this.props.children

    return (
      <main className="login-page">
        <section className="login-panel compact-panel" aria-labelledby="application-error-title">
          <p className="technical-label">APPLICATION RECOVERY</p>
          <h1 id="application-error-title">Controller interface interrupted</h1>
          <div className="error-summary" role="alert">
            The interface could not continue. Retry without leaving the Controller.
          </div>
          <button
            className="primary-button"
            type="button"
            ref={this.retryRef}
            onClick={() => this.setState({ interrupted: false })}
          >
            Retry application
          </button>
        </section>
      </main>
    )
  }
}
