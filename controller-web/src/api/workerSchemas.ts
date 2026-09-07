import { z } from "zod"

import { taskProgressSchema } from "./taskSchemas"

const unsignedIntegerSchema = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER)
const positiveU16Schema = z.number().int().min(1).max(65_535)

export const workerApiUrlSchema = z.string().refine((value) => {
  if (!URL.canParse(value)) return false
  const url = new URL(value)
  return (url.protocol === "http:" || url.protocol === "https:")
    && url.username === ""
    && url.password === ""
    && url.search === ""
    && url.hash === ""
}, "Enter a credential-free HTTP(S) base URL without a query or fragment.")

const workerPasswordSchema = z.string().refine((value) => value.length > 0 && new TextEncoder().encode(value).length <= 1024 && !/\p{Cc}/u.test(value), "Enter 1 to 1024 UTF-8 bytes without control characters.")

const workerCreateBaseSchema = z
  .object({
    name: z.string().refine((value) => value.trim().length > 0, "Enter a worker name."),
    api_url: workerApiUrlSchema.optional(),
    transport: z.enum(["http", "iroh"]).optional(),
    endpoint_id: z.string().regex(/^[0-9a-fA-F]{64}$/, "Enter the 64-character Endpoint ID, not a ticket.").optional(),
    enabled: z.boolean(),
    compute_slots: positiveU16Schema,
    password: workerPasswordSchema.optional(),
  })
  .strict()

function validateEndpoint(value: { transport?: string | undefined; api_url?: string | undefined; endpoint_id?: string | undefined }, ctx: z.RefinementCtx) {
  if (value.transport === "iroh") {
    if (!value.endpoint_id || value.api_url !== undefined) ctx.addIssue({ code: "custom", path: ["endpoint_id"], message: "Provide an Endpoint ID without an HTTP URL." })
  } else if (!value.api_url || value.endpoint_id !== undefined) {
    ctx.addIssue({ code: "custom", path: ["api_url"], message: "Provide an HTTP(S) URL." })
  }
}
export const workerCreateRequestSchema = workerCreateBaseSchema.superRefine((value, ctx) => {
  validateEndpoint(value, ctx)
  if (value.transport === "iroh" && !value.password) ctx.addIssue({ code: "custom", path: ["password"], message: "Iroh requires the worker password." })
})
export const workerUpdateRequestSchema = workerCreateBaseSchema
  .extend({ version: unsignedIntegerSchema, password: workerPasswordSchema.nullable().optional() })
  .strict().superRefine(validateEndpoint)

const workflowSummarySchema = z
  .object({
    name: z.string(),
    kind: z.enum(["workflow", "preset"]),
  })
  .strict()

const workerCapacitySchema = z
  .object({
    used_slots: z.number().int().nonnegative().max(65_535),
    available_slots: z.number().int().nonnegative().max(65_535),
    assigned_tasks: z.number().int().nonnegative().max(4_294_967_295),
    staged_tasks: z.number().int().nonnegative().max(4_294_967_295),
    processing_tasks: z.number().int().nonnegative().max(4_294_967_295),
    active_uploads: z.number().int().nonnegative().max(65_535),
    active_downloads: z.number().int().nonnegative().max(65_535),
    progress: taskProgressSchema.nullable(),
  })
  .strict()

export const workerSchema = z
  .object({
    id: z.string().uuid(),
    version: unsignedIntegerSchema,
    name: z.string(),
    api_url: workerApiUrlSchema.optional(),
    transport: z.enum(["http", "iroh"]).optional(),
    endpoint_id: z.string().regex(/^[0-9a-fA-F]{64}$/, "Enter the 64-character Endpoint ID, not a ticket.").optional(),
    enabled: z.boolean(),
    online: z.boolean(),
    compute_slots: positiveU16Schema,
    capabilities: z
      .object({
        workflows: z.array(workflowSummarySchema),
        refreshed_at: z.iso.datetime().nullable(),
      })
      .strict(),
    capacity: workerCapacitySchema,
    last_seen_at: z.iso.datetime().nullable(),
    last_assigned_at: z.iso.datetime().nullable(),
    created_at: z.iso.datetime(),
    updated_at: z.iso.datetime(),
    last_error: z.string().nullable(),
    has_password: z.boolean().optional(),
  })
  .strict().superRefine(validateEndpoint)

export const workerListSchema = z
  .object({
    items: z.array(workerSchema),
    total: unsignedIntegerSchema,
  })
  .strict()

export const workerDeleteResponseSchema = z
  .object({
    worker_id: z.string().uuid(),
    deleted: z.boolean(),
  })
  .strict()

export const workerUpdatedEventSchema = z
  .object({
    type: z.literal("worker_updated"),
    data: z.object({ event_id: z.string().uuid(), worker: workerSchema }).strict(),
  })
  .strict()

export type Worker = z.infer<typeof workerSchema>
export type WorkerCreateRequest = z.infer<typeof workerCreateRequestSchema>
export type WorkerList = z.infer<typeof workerListSchema>
export type WorkerUpdateRequest = z.infer<typeof workerUpdateRequestSchema>
