import { useLayoutEffect, useRef } from "react"

import { Button } from "../ui/Button"

type BootstrapErrorProps = {
  readonly message: string
  readonly retry: () => void
}

export function BootstrapError({ message, retry }: BootstrapErrorProps) {
  const alertRef = useRef<HTMLDivElement>(null)

  useLayoutEffect(() => alertRef.current?.focus(), [])

  return (
    <main className="login-page">
      <section className="login-panel compact-panel" aria-labelledby="unavailable-title">
        <p className="technical-label">CONTROLLER CHECK</p>
        <h1 id="unavailable-title">Controller unavailable</h1>
        <div className="error-summary" role="alert" tabIndex={-1} ref={alertRef}>
          {message}
        </div>
        <Button variant="primary" onClick={retry}>
          Retry Controller check
        </Button>
      </section>
    </main>
  )
}
