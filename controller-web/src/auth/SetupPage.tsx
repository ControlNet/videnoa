import { LockKeyhole } from "lucide-react"
import { type FormEvent, useLayoutEffect, useRef, useState } from "react"

import { setupRequestSchema } from "../api/schemas"
import type { SetupResult } from "./useSessionController"
import "./auth.css"

type SetupPageProps = {
  readonly setup: (request: { readonly password: string; readonly password_confirmation: string }) => Promise<SetupResult>
}

type SetupError = {
  readonly field: "password" | "confirmation" | "summary"
  readonly generation: number
  readonly message: string
}

const setupErrorId = "setup-error-summary"
const passwordErrorId = "setup-password-error"
const confirmationErrorId = "setup-confirmation-error"

export function SetupPage({ setup }: SetupPageProps) {
  const [password, setPassword] = useState("")
  const [confirmation, setConfirmation] = useState("")
  const [error, setError] = useState<SetupError | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const passwordRef = useRef<HTMLInputElement>(null)
  const confirmationRef = useRef<HTMLInputElement>(null)
  const alertRef = useRef<HTMLDivElement>(null)
  const submitGenerationRef = useRef(0)

  useLayoutEffect(() => {
    passwordRef.current?.focus()
    return () => {
      submitGenerationRef.current += 1
    }
  }, [])

  useLayoutEffect(() => {
    if (error?.field === "password") passwordRef.current?.focus()
    if (error?.field === "confirmation") confirmationRef.current?.focus()
    if (error?.field === "summary") alertRef.current?.focus()
  }, [error])

  async function handleSubmit(event: FormEvent<HTMLFormElement>): Promise<void> {
    event.preventDefault()
    const generation = submitGenerationRef.current + 1
    submitGenerationRef.current = generation
    setError(null)
    const parsed = setupRequestSchema.safeParse({ password, password_confirmation: confirmation })
    if (!parsed.success) {
      const issue = parsed.error.issues[0]
      if (issue === undefined) return
      setError({
        field: issue.path[0] === "password_confirmation" ? "confirmation" : "password",
        generation,
        message: issue.message,
      })
      return
    }

    setSubmitting(true)
    const result = await setup(parsed.data)
    if (generation !== submitGenerationRef.current || result.ok) return
    setSubmitting(false)
    setError({ field: "summary", generation, message: result.message })
  }

  return (
    <main className="login-page">
      <section className="login-panel" aria-labelledby="setup-title">
        <div className="login-brand" aria-hidden="true">
          <span className="brand-mark">V</span>
          <span className="boundary-line" />
        </div>
        <p className="technical-label">VIDENOA / FIRST ACCESS</p>
        <h1 id="setup-title">Set up Controller access</h1>
        <p className="login-summary">Create the administrator password for this private coordination surface.</p>

        <form className="login-form" noValidate onSubmit={handleSubmit}>
          {error?.field === "summary" ? (
            <div id={setupErrorId} className="error-summary" role="alert" tabIndex={-1} ref={alertRef}>
              {error.message}
            </div>
          ) : null}
          <label htmlFor="setup-password">Create password</label>
          <div className={`input-frame${error?.field === "password" ? " invalid" : ""}`}>
            <LockKeyhole size={17} strokeWidth={1.75} aria-hidden="true" />
            <input
              id="setup-password"
              type="password"
              autoComplete="new-password"
              ref={passwordRef}
              value={password}
              aria-invalid={error?.field === "password" ? true : undefined}
              aria-describedby={error?.field === "password" ? passwordErrorId : undefined}
              onChange={(event) => setPassword(event.currentTarget.value)}
              disabled={submitting}
              required
            />
          </div>
          {error?.field === "password" ? <small id={passwordErrorId} className="setup-field-error" role="alert">{error.message}</small> : null}

          <label htmlFor="setup-confirmation">Confirm password</label>
          <div className={`input-frame${error?.field === "confirmation" ? " invalid" : ""}`}>
            <LockKeyhole size={17} strokeWidth={1.75} aria-hidden="true" />
            <input
              id="setup-confirmation"
              type="password"
              autoComplete="new-password"
              ref={confirmationRef}
              value={confirmation}
              aria-invalid={error?.field === "confirmation" ? true : undefined}
              aria-describedby={error?.field === "confirmation" ? confirmationErrorId : undefined}
              onChange={(event) => setConfirmation(event.currentTarget.value)}
              disabled={submitting}
              required
            />
          </div>
          {error?.field === "confirmation" ? <small id={confirmationErrorId} className="setup-field-error" role="alert">{error.message}</small> : null}

          <button className="primary-button" type="submit" disabled={submitting}>
            {submitting ? "Creating access..." : "Create secure access"}
          </button>
        </form>

        <p className="login-footnote">The password stays in this request and is never written to browser storage.</p>
      </section>
    </main>
  )
}
