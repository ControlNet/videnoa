import { z } from "zod"

import { ApiClientError } from "../api/client"
import { taskCreateRequestSchema } from "../api/taskSchemas"
import type { SubmissionIntent } from "./submissionIntent"

export const batchPreviewSchema = z.object({
  items: z.array(z.object({ request: taskCreateRequestSchema, error: z.string().nullable(),
    validation_error: z.string().nullable().optional(), output_key: z.string().optional() }).strict()).max(500),
}).strict()

export type BatchOptions = {
  input_pattern: string
  output_mode: "beside_input" | "directory"
  output_directory: string
  naming_mode: "insert_extension" | "jellyfin_version_suffix" | "original"
  middle_extension: string
  workflow: string
  priority: string
}

export const initialBatchOptions: BatchOptions = {
  input_pattern: "", output_mode: "beside_input", output_directory: "",
  naming_mode: "insert_extension", middle_extension: "AI", workflow: "", priority: "0",
}

export type BatchRow = z.infer<typeof batchPreviewSchema>["items"][number] & {
  excluded: boolean
  intent: SubmissionIntent
  status: "ready" | "creating" | "created" | "failed"
  submissionError: string | null
}

export function batchError(error: unknown): string {
  if (error instanceof ApiClientError) {
    return error.fieldErrors.length > 0
      ? error.fieldErrors.map((field) => field.message).join(" ")
      : error.code === "network_failure" ? "Response lost. Retry uses the same task key." : error.message
  }
  return "The request did not finish. Retry uses the same task key."
}


export function batchRowErrors(rows: readonly BatchRow[]): ReadonlyMap<string, string | null> {
  const outputCounts = new Map<string, number>()
  for (const row of rows) {
    if (row.excluded) continue
    const key = row.output_key ?? row.request.output_path
    outputCounts.set(key, (outputCounts.get(key) ?? 0) + 1)
  }
  return new Map(rows.map((row) => {
    // Older servers supply only error; never discard an unclassified safety error.
    const validation = row.validation_error === undefined ? row.error : row.validation_error
    const duplicate = (outputCounts.get(row.output_key ?? row.request.output_path) ?? 0) > 1
    return [row.request.input_path, row.excluded ? null : validation ?? (duplicate ? "Multiple inputs map to this output path." : null)]
  }))
}
