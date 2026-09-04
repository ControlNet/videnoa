import { useCallback, useEffect, useMemo, useState } from "react"

import { type ApiClient, ApiClientError, createApiClient } from "../api/client"
import {
  loginResponseSchema,
  logoutResponseSchema,
  type Session,
  sessionSchema,
} from "../api/schemas"

export type AuthState =
  | { readonly kind: "checking" }
  | { readonly kind: "unauthenticated" }
  | { readonly kind: "authenticated"; readonly session: Session }
  | { readonly kind: "bootstrap_error"; readonly message: string }

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

export type SessionController = {
  readonly apiClient: ApiClient
  readonly login: (password: string) => Promise<LoginResult>
  readonly logout: () => Promise<LogoutResult>
  readonly retryBootstrap: () => void
  readonly state: AuthState
}

export function useSessionController(): SessionController {
  const [state, setState] = useState<AuthState>({ kind: "checking" })
  const apiClient = useMemo(
    () =>
      createApiClient({
        fetcher: globalThis.fetch,
        onUnauthorized: () => setState({ kind: "unauthenticated" }),
      }),
    [],
  )

  const checkSession = useCallback(async (): Promise<AuthState> => {
    try {
      const session = await apiClient.request("api/auth/session", { schema: sessionSchema })
      return { kind: "authenticated", session }
    } catch (error) {
      if (!(error instanceof ApiClientError)) throw error
      if (error.code === "unauthorized") {
        return { kind: "unauthenticated" }
      }
      return { kind: "bootstrap_error", message: messageFor(error) }
    }
  }, [apiClient])

  useEffect(() => {
    void checkSession().then(setState)
  }, [checkSession])

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
      setState({ kind: "unauthenticated" })
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
      void checkSession().then(setState)
    },
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
    case "http_error":
    case "internal":
    case "internal_error":
    case "invalid_request":
    case "not_found":
    case "conflict":
    case "publication_ambiguous":
    case "remote_state_ambiguous":
    case "unavailable":
      return "Controller could not complete the request."
  }
}
