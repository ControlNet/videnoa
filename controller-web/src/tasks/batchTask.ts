import { z } from "zod"

import { ApiClientError } from "../api/client"
import { taskCreateRequestSchema } from "../api/taskSchemas"
import type { SubmissionIntent } from "./submissionIntent"

export const batchPreviewSchema = z.object({
  items: z.array(z.object({ request: taskCreateRequestSchema, error: z.string().nullable() }).strict()).max(500),
}).strict()

export type BatchOptions = {
  input_pattern: string
  output_mode: "beside_input" | "directory"
  output_directory: string
  naming_mode: "insert_extension" | "original"
  middle_extension: string
  workflow: string
  priority: string
}

export const initialBatchOptions: BatchOptions = {
  input_pattern: "", output_mode: "beside_input", output_directory: "",
  naming_mode: "insert_extension", middle_extension: "AI", workflow: "", priority: "0",
}

export type BatchRow = z.infer<typeof batchPreviewSchema>["items"][number] & {
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
