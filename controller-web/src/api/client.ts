import ky from "ky"
import type { ZodType } from "zod"

import { apiErrorSchema, type FieldErrorCode, type ServerApiErrorCode } from "./schemas"

export type ApiErrorCode = ServerApiErrorCode | "http_error" | "malformed_response" | "network_failure"

export class ApiClientError extends Error {
  readonly name = "ApiClientError"
  readonly code: ApiErrorCode
  readonly status: number | null
  readonly retryable: boolean
  readonly fieldErrors: readonly ApiFieldError[]

  constructor(code: ApiErrorCode, status: number | null = null, message: string = code, retryable = false, fieldErrors: readonly ApiFieldError[] = []) {
    super(message)
    this.code = code
    this.status = status
    this.retryable = retryable
    this.fieldErrors = fieldErrors
  }
}

export type ApiFieldError = {
  readonly field: string
  readonly code: FieldErrorCode
  readonly message: string
}

type ClientOptions = {
  readonly fetcher: typeof fetch
  readonly onUnauthorized: () => void
}

type RequestMethod = "DELETE" | "GET" | "PATCH" | "POST" | "PUT"

type TransportOptions = {
  readonly json?: unknown
  readonly headers?: Readonly<Record<string, string>>
  readonly signal?: AbortSignal
}

type RequestOptions<T> = TransportOptions & {
  readonly schema: ZodType<T>
  readonly method?: RequestMethod
}

export type ApiClient = {
  readonly clearCsrfProof: () => void
  readonly csrfProof: () => string | null
  readonly request: <T>(path: string, options: RequestOptions<T>) => Promise<T>
}

export function createApiClient(options: ClientOptions): ApiClient {
  let csrfProof: string | null = null
  let proofRefresh: Promise<string | null> | null = null
  const transport = ky.create({
    credentials: "same-origin",
    fetch: (request) => Reflect.apply(options.fetcher, globalThis, [request]),
    prefixUrl: window.location.origin,
    retry: 0,
    throwHttpErrors: false,
    timeout: 15_000,
  })

  async function send(path: string, method: RequestMethod, requestOptions: TransportOptions): Promise<Response> {
    const headers = new Headers()
    for (const [name, value] of Object.entries(requestOptions.headers ?? {})) headers.set(name, value)
    if (method !== "GET" && csrfProof !== null) {
      headers.set("x-csrf-token", csrfProof)
    }

    let response: Response
    try {
      response = await transport(path, {
        headers,
        json: requestOptions.json,
        method,
        ...(requestOptions.signal === undefined ? {} : { signal: requestOptions.signal }),
      })
    } catch (error) {
      if (error instanceof TypeError || error instanceof DOMException) {
        throw new ApiClientError("network_failure", null)
      }
      throw error
    }

    const rotatedProof = response.headers.get("x-csrf-token")
    if (rotatedProof !== null) csrfProof = rotatedProof
    return response
  }

  async function failure(response: Response): Promise<ApiClientError> {
    if (response.status === 401) {
      csrfProof = null
      options.onUnauthorized()
      return new ApiClientError("unauthorized", response.status)
    }
    const parsedError = apiErrorSchema.safeParse(await readJson(response))
    if (!parsedError.success) return new ApiClientError("malformed_response", response.status)
    return new ApiClientError(parsedError.data.code, response.status, parsedError.data.message, parsedError.data.retryable, parsedError.data.fieldErrors)
  }

  /*
   * The Controller keeps one CSRF digest per session and rotates it on every
   * `GET /api/auth/session`. All tabs of one browser share the session cookie,
   * so a second tab bootstrapping silently invalidates the proof this tab
   * holds and its next mutation is rejected with 403 `forbidden`. Recover by
   * fetching a fresh proof -- one refetch shared by every mutation rejected
   * while it is in flight -- so the caller can retry exactly once.
   */
  function refreshProof(): Promise<string | null> {
    proofRefresh ??= (async () => {
      try {
        const response = await send("api/auth/session", "GET", {})
        if (response.status === 401) throw await failure(response)
        return response.ok ? response.headers.get("x-csrf-token") : null
      } finally {
        proofRefresh = null
      }
    })()
    return proofRefresh
  }

  function isStaleProofRejection(method: string, response: Response, error: ApiClientError) {
    return method !== "GET" && response.status === 403 && error.code === "forbidden"
  }

  return {
    clearCsrfProof: () => {
      csrfProof = null
    },
    csrfProof: () => csrfProof,
    request: async <T>(path: string, requestOptions: RequestOptions<T>): Promise<T> => {
      const method = requestOptions.method ?? "GET"
      let response = await send(path, method, requestOptions)

      if (!response.ok) {
        const error = await failure(response)
        if (!isStaleProofRejection(method, response, error) || (await refreshProof()) === null) throw error
        response = await send(path, method, requestOptions)
        if (!response.ok) throw await failure(response)
      }

      const parsed = requestOptions.schema.safeParse(await readJson(response))
      if (!parsed.success) throw new ApiClientError("malformed_response", response.status)
      return parsed.data
    },
  }
}


async function readJson(response: Response): Promise<unknown> {
  try {
    return await response.json()
  } catch (error) {
    if (error instanceof SyntaxError) throw new ApiClientError("malformed_response", response.status)
    throw error
  }
}
