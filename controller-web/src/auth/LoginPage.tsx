import { LockKeyhole } from "lucide-react"
import { type FormEvent, useLayoutEffect, useRef, useState } from "react"

import type { LoginResult } from "./useSessionController"
import "./auth.css"

type LoginPageProps = {
  readonly login: (password: string) => Promise<LoginResult>
}

type LoginError = Extract<LoginResult, { readonly ok: false }> & { readonly generation: number }

const loginErrorId = "login-error-summary"

export function LoginPage({ login }: LoginPageProps) {
  const [password, setPassword] = useState("")
  const [error, setError] = useState<LoginError | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const alertRef = useRef<HTMLDivElement>(null)
  const passwordRef = useRef<HTMLInputElement>(null)
  const submitGenerationRef = useRef(0)

  useLayoutEffect(() => {
    passwordRef.current?.focus()
    return () => {
      submitGenerationRef.current += 1
    }
  }, [])

  useLayoutEffect(() => {
    if (error !== null) alertRef.current?.focus()
  }, [error])

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const generation = submitGenerationRef.current + 1
    submitGenerationRef.current = generation
    setError(null)
    setSubmitting(true)
    const result = await login(password)
    if (generation !== submitGenerationRef.current || result.ok) return
    setSubmitting(false)
    setError({ ...result, generation })
  }

  return (
    <main className="login-page">
      <section className="login-panel" aria-labelledby="login-title">
        <div className="login-brand" aria-hidden="true">
          <span className="brand-mark">V</span>
          <span className="boundary-line" />
        </div>
        <p className="technical-label">VIDENOA / CONTROL PLANE</p>
        <h1 id="login-title">Sign in to Controller</h1>
        <p className="login-summary">
          Open the private coordination surface for tasks, workers, and runtime settings.
        </p>

        <form className="login-form" onSubmit={handleSubmit}>
          {error === null ? null : (
            <div id={loginErrorId} className="error-summary" role="alert" tabIndex={-1} ref={alertRef}>
              {error.message}
            </div>
          )}
          <label htmlFor="controller-password">Controller password</label>
          <div className="input-frame">
            <LockKeyhole size={17} strokeWidth={1.75} aria-hidden="true" />
            <input
              id="controller-password"
              type="password"
              autoComplete="current-password"
              ref={passwordRef}
              value={password}
              aria-invalid={error?.kind === "invalid_credentials" ? true : undefined}
              aria-describedby={error?.kind === "invalid_credentials" ? loginErrorId : undefined}
              onChange={(event) => setPassword(event.currentTarget.value)}
              disabled={submitting}
              required
            />
          </div>
          <button className="primary-button" type="submit" disabled={submitting}>
            {submitting ? "Signing in..." : "Sign in"}
          </button>
        </form>

        <p className="login-footnote">Credentials stay in this request and the HttpOnly session cookie.</p>
      </section>
    </main>
  )
}
