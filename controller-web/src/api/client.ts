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

type RequestOptions<T> = {
  readonly schema: ZodType<T>
  readonly method?: "DELETE" | "GET" | "PATCH" | "POST" | "PUT"
  readonly json?: unknown
  readonly headers?: Readonly<Record<string, string>>
  readonly signal?: AbortSignal
}

export type ApiClient = {
  readonly clearCsrfProof: () => void
  readonly csrfProof: () => string | null
  readonly request: <T>(path: string, options: RequestOptions<T>) => Promise<T>
}

export function createApiClient(options: ClientOptions): ApiClient {
  let csrfProof: string | null = null
  const transport = ky.create({
    credentials: "same-origin",
    fetch: (request) => Reflect.apply(options.fetcher, globalThis, [request]),
    prefixUrl: window.location.origin,
    retry: 0,
    throwHttpErrors: false,
    timeout: 15_000,
  })

  return {
    clearCsrfProof: () => {
      csrfProof = null
    },
    csrfProof: () => csrfProof,
    request: async <T>(path: string, requestOptions: RequestOptions<T>): Promise<T> => {
      const method = requestOptions.method ?? "GET"
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

      if (!response.ok) {
        if (response.status === 401) {
          csrfProof = null
          options.onUnauthorized()
          throw new ApiClientError("unauthorized", response.status)
        }
        const parsedError = apiErrorSchema.safeParse(await readJson(response))
        if (!parsedError.success) throw new ApiClientError("malformed_response", response.status)
        throw new ApiClientError(parsedError.data.code, response.status, parsedError.data.message, parsedError.data.retryable, parsedError.data.fieldErrors)
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
