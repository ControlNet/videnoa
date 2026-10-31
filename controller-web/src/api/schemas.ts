import { z } from "zod"

/*
 * Response schemas are not strict: a field a newer Controller adds is dropped,
 * so a Controller upgraded before the browser reloads does not break the UI.
 * Request schemas stay strict, since this UI builds them.
 */

/**
 * A list a newer Controller may extend: each row is parsed on its own, so a
 * row this UI cannot read (say, a status it does not know yet) is left out
 * instead of failing the whole list.
 */
export function rowsOf<Row extends z.ZodType>(row: Row) {
  return z.array(z.unknown()).transform((rows): z.output<Row>[] =>
    rows.flatMap((value) => {
      const parsed = row.safeParse(value)
      if (parsed.success) return [parsed.data]
      console.warn("Left out a list row this page cannot read.", parsed.error.issues)
      return []
    }),
  )
}

export const sessionSchema = z
  .object({
    id: z.string().uuid(),
    authenticated: z.literal(true),
    method: z.enum(["session", "bearer"]),
    expires_at: z.iso.datetime(),
    idle_expires_at: z.iso.datetime(),
  })

export const loginResponseSchema = z.object({ session: sessionSchema })
export const logoutResponseSchema = z.object({ logged_out: z.literal(true) })
export const setupStatusSchema = z.object({ initialized: z.boolean() })

/*
 * Narrowed to HTTP(S) before it can reach an `href`. The Controller sends a
 * compile-time constant today, but a schema is the only place that guarantee is
 * enforced, and a scheme check is what keeps a `javascript:` URL out of the DOM
 * if that ever stops being true.
 */
const sourceUrlSchema = z.string().refine((value) => {
  if (!URL.canParse(value)) return false
  const url = new URL(value)
  return url.protocol === "http:" || url.protocol === "https:"
}, "Expected an HTTP(S) source URL.")

export const aboutSchema = z
  .object({
    name: z.string().min(1),
    version: z.string().min(1),
    source_url: sourceUrlSchema,
  })

export type About = z.infer<typeof aboutSchema>

const setupPasswordSchema = z.string().superRefine((password, context) => {
  const byteLength = new TextEncoder().encode(password).byteLength
  if (byteLength < 12) {
    context.addIssue({ code: "custom", message: "Use at least 12 UTF-8 bytes." })
  } else if (byteLength > 1024) {
    context.addIssue({ code: "custom", message: "Use at most 1024 UTF-8 bytes." })
  }
})

export const setupRequestSchema = z
  .object({
    password: setupPasswordSchema,
    password_confirmation: z.string(),
  })
  .strict()
  .superRefine((request, context) => {
    if (request.password !== request.password_confirmation) {
      context.addIssue({ code: "custom", path: ["password_confirmation"], message: "Passwords do not match." })
    }
  })

const authErrorCodeSchema = z.enum(["unauthorized", "forbidden", "invalid_request", "conflict", "rate_limited", "internal"])
const operationsErrorCodeSchema = z.enum([
  "unauthorized",
  "forbidden",
  "invalid_request",
  "not_found",
  "conflict",
  "rate_limited",
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
            }),
        ),
      }),
  })
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
export type SetupRequest = z.infer<typeof setupRequestSchema>
