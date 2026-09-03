import { useEffect, useRef } from "react"

type BootstrapErrorProps = {
  readonly message: string
  readonly retry: () => void
}

export function BootstrapError({ message, retry }: BootstrapErrorProps) {
  const alertRef = useRef<HTMLDivElement>(null)

  useEffect(() => alertRef.current?.focus(), [])

  return (
    <main className="login-page">
      <section className="login-panel compact-panel" aria-labelledby="unavailable-title">
        <p className="technical-label">SESSION CHECK</p>
        <h1 id="unavailable-title">Controller unavailable</h1>
        <div className="error-summary" role="alert" tabIndex={-1} ref={alertRef}>
          {message}
        </div>
        <button className="primary-button" type="button" onClick={retry}>
          Retry session check
        </button>
      </section>
    </main>
  )
}
