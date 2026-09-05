import { useCallback, useEffect, useMemo, useState } from "react"

import { type ApiClient, ApiClientError, createApiClient } from "../api/client"
import {
  loginResponseSchema,
  logoutResponseSchema,
  type Session,
  type SetupRequest,
  sessionSchema,
  setupStatusSchema,
} from "../api/schemas"

export type AuthState =
  | { readonly kind: "checking" }
  | { readonly kind: "setup_required" }
  | { readonly kind: "unauthenticated"; readonly notice: string | null }
  | { readonly kind: "authenticated"; readonly session: Session }
  | { readonly kind: "bootstrap_error"; readonly message: string }

type SettledAuthState = Exclude<AuthState, { readonly kind: "checking" }>

export type LoginResult =
  | { readonly ok: true }
  | {
      readonly ok: false
      readonly kind: "invalid_credentials" | "recovery"
      readonly message: string
    }

export type LogoutResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly message: string }

export type SetupResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly message: string }

export type SessionController = {
  readonly apiClient: ApiClient
  readonly login: (password: string) => Promise<LoginResult>
  readonly logout: () => Promise<LogoutResult>
  readonly retryBootstrap: () => void
  readonly setup: (request: SetupRequest) => Promise<SetupResult>
  readonly state: AuthState
}

export function useSessionController(): SessionController {
  const [state, setState] = useState<AuthState>({ kind: "checking" })
  const apiClient = useMemo(
    () =>
      createApiClient({
        fetcher: globalThis.fetch,
        onUnauthorized: () => setState({ kind: "unauthenticated", notice: null }),
      }),
    [],
  )

  const checkSession = useCallback(async (): Promise<SettledAuthState> => {
    try {
      const session = await apiClient.request("api/auth/session", { schema: sessionSchema })
      return { kind: "authenticated", session }
    } catch (error) {
      if (!(error instanceof ApiClientError)) throw error
      if (error.code === "unauthorized") {
        return { kind: "unauthenticated", notice: null }
      }
      return { kind: "bootstrap_error", message: messageFor(error) }
    }
  }, [apiClient])

  const checkBootstrap = useCallback(async (): Promise<SettledAuthState> => {
    try {
      const setupStatus = await apiClient.request("api/auth/setup", { schema: setupStatusSchema })
      if (!setupStatus.initialized) return { kind: "setup_required" }
      return checkSession()
    } catch (error) {
      if (!(error instanceof ApiClientError)) throw error
      return { kind: "bootstrap_error", message: messageFor(error) }
    }
  }, [apiClient, checkSession])

  useEffect(() => {
    void checkBootstrap().then(setState)
  }, [checkBootstrap])

  const setup = useCallback(
    async (request: SetupRequest): Promise<SetupResult> => {
      try {
        const response = await apiClient.request("api/auth/setup", {
          json: request,
          method: "POST",
          schema: loginResponseSchema,
        })
        setState({ kind: "authenticated", session: response.session })
        return { ok: true }
      } catch (error) {
        if (!(error instanceof ApiClientError)) throw error
        if (error.status === 409) {
          const nextState = await checkBootstrap()
          switch (nextState.kind) {
            case "authenticated":
              setState(nextState)
              return { ok: true }
            case "unauthenticated": {
              const message = "Controller setup was completed elsewhere. Sign in with the administrator password."
              setState({ kind: "unauthenticated", notice: message })
              return { ok: false, message }
            }
            case "setup_required":
              setState(nextState)
              return { ok: false, message: "Controller setup is still incomplete. Try again." }
            case "bootstrap_error":
              setState(nextState)
              return { ok: false, message: nextState.message }
            default: {
              const unreachableState: never = nextState
              return unreachableState
            }
          }
        }
        return { ok: false, message: messageFor(error) }
      }
    },
    [apiClient, checkBootstrap],
  )

  const login = useCallback(
    async (password: string): Promise<LoginResult> => {
      try {
        const response = await apiClient.request("api/auth/login", {
          json: { password },
          method: "POST",
          schema: loginResponseSchema,
        })
        setState({ kind: "authenticated", session: response.session })
        return { ok: true }
      } catch (error) {
        if (error instanceof ApiClientError) {
          return {
            ok: false,
            kind: error.code === "unauthorized" ? "invalid_credentials" : "recovery",
            message: messageFor(error),
          }
        }
        throw error
      }
    },
    [apiClient],
  )

  const logout = useCallback(async (): Promise<LogoutResult> => {
    try {
      await apiClient.request("api/auth/logout", {
        method: "POST",
        schema: logoutResponseSchema,
      })
      apiClient.clearCsrfProof()
      setState({ kind: "unauthenticated", notice: null })
      return { ok: true }
    } catch (error) {
      if (error instanceof ApiClientError) {
        return { ok: false, message: "Controller could not complete sign out. Try again." }
      }
      throw error
    }
  }, [apiClient])

  return {
    apiClient,
    login,
    logout,
    retryBootstrap: () => {
      setState({ kind: "checking" })
      void checkBootstrap().then(setState)
    },
    setup,
    state,
  }
}

function messageFor(error: ApiClientError): string {
  switch (error.code) {
    case "unauthorized":
      return "The password was not accepted."
    case "malformed_response":
      return "Controller returned an invalid response."
    case "network_failure":
      return "Controller could not be reached."
    case "rate_limited":
      return "Too many sign-in attempts. Try again shortly."
    case "forbidden":
      return "Controller rejected the request proof."
    case "invalid_request":
      return "Controller rejected the setup values."
    case "http_error":
    case "internal":
    case "internal_error":
    case "not_found":
    case "conflict":
    case "publication_ambiguous":
    case "remote_state_ambiguous":
    case "unavailable":
      return "Controller could not complete the request."
  }
}
