import { z } from "zod"

import type { ApiClient } from "../api/client"
import { workerListSchema } from "../api/workerSchemas"

export const taskSuggestionsSchema = z.object({
  items: z.array(z.object({ value: z.string(), kind: z.enum(["directory", "file", "workflow", "preset"]) }).strict()),
  truncated: z.boolean(),
}).strict()

export type TaskSuggestion = z.infer<typeof taskSuggestionsSchema>["items"][number]
export type TaskSuggestionField = "input_path" | "output_path" | "workflow"

export async function loadTaskSuggestions(apiClient: ApiClient, name: TaskSuggestionField, value: string, signal: AbortSignal) {
  if (name !== "workflow") {
    const query = new URLSearchParams({ prefix: value, kind: name === "input_path" ? "input" : "output" })
    return apiClient.request(`api/task-path-suggestions?${query}`, { schema: taskSuggestionsSchema, signal })
  }
  const workers = await apiClient.request("api/workers", { schema: workerListSchema, signal })
  const workflows = new Map<string, TaskSuggestion>()
  for (const worker of workers.items) {
    if (!worker.enabled) continue
    for (const workflow of worker.capabilities.workflows) {
      if (workflow.name.toLowerCase().includes(value.toLowerCase()) && !workflows.has(workflow.name)) {
        workflows.set(workflow.name, { value: workflow.name, kind: workflow.kind })
      }
    }
  }
  const items = [...workflows.values()].sort((left, right) => left.value.localeCompare(right.value))
  return { items: items.slice(0, 100), truncated: items.length > 100 }
}
