import { z } from "zod"

export const sessionSchema = z
  .object({
    id: z.string().uuid(),
    authenticated: z.literal(true),
    method: z.enum(["session", "bearer"]),
    expires_at: z.iso.datetime(),
    idle_expires_at: z.iso.datetime(),
  })
  .strict()

export const loginResponseSchema = z.object({ session: sessionSchema }).strict()
export const logoutResponseSchema = z.object({ logged_out: z.literal(true) }).strict()

const authErrorCodeSchema = z.enum(["unauthorized", "forbidden", "rate_limited", "internal"])
const operationsErrorCodeSchema = z.enum([
  "unauthorized",
  "forbidden",
  "invalid_request",
  "not_found",
  "conflict",
  "remote_state_ambiguous",
  "publication_ambiguous",
  "unavailable",
  "internal_error",
])
export const fieldErrorCodeSchema = z.enum(["required", "invalid_value", "unknown_value", "out_of_range", "conflict"])

export type ServerApiErrorCode = z.infer<typeof authErrorCodeSchema> | z.infer<typeof operationsErrorCodeSchema>
export type FieldErrorCode = z.infer<typeof fieldErrorCodeSchema>

type ParsedApiError = {
  readonly code: ServerApiErrorCode
  readonly message: string
  readonly retryable: boolean
  readonly fieldErrors: readonly {
    readonly field: string
    readonly code: FieldErrorCode
    readonly message: string
  }[]
}

const flatApiErrorSchema = z
  .object({
    error: authErrorCodeSchema,
  })
  .strict()
  .transform(({ error }): ParsedApiError => ({ code: error, message: error, retryable: false, fieldErrors: [] }))

const nestedApiErrorSchema = z
  .object({
    error: z
      .object({
        code: operationsErrorCodeSchema,
        message: z.string().min(1),
        retryable: z.boolean(),
        field_errors: z.array(
          z
            .object({
              field: z.string(),
              code: fieldErrorCodeSchema,
              message: z.string(),
            })
            .strict(),
        ),
      })
      .strict(),
  })
  .strict()
  .transform(
    ({ error }): ParsedApiError => ({
      code: error.code,
      message: error.message,
      retryable: error.retryable,
      fieldErrors: error.field_errors,
    }),
  )

export const apiErrorSchema = z.union([flatApiErrorSchema, nestedApiErrorSchema])

export type Session = z.infer<typeof sessionSchema>
