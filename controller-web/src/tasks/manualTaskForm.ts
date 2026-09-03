import type { ApiClientError } from "../api/client"

export type ManualTaskFields = {
  readonly inputPath: string
  readonly outputPath: string
  readonly workflow: string
  readonly priority: string
}

export function manualTaskFieldErrors(error: ApiClientError): Partial<Record<keyof ManualTaskFields, string>> {
  const result: Partial<Record<keyof ManualTaskFields, string>> = {}
  for (const fieldError of error.fieldErrors) {
    if (fieldError.field === "input_path") result.inputPath = fieldError.message
    if (fieldError.field === "output_path") result.outputPath = fieldError.message
    if (fieldError.field === "workflow") result.workflow = fieldError.message
    if (fieldError.field === "priority") result.priority = fieldError.message
  }
  return result
}

export function manualTaskErrorMessage(error: ApiClientError): string {
  const normalized = error.message.toLocaleLowerCase()
  const outputCollision = error.fieldErrors.some((fieldError) => {
    const message = fieldError.message.toLocaleLowerCase()
    return fieldError.field === "output_path"
      && (fieldError.code === "invalid_value" || fieldError.code === "conflict")
      && message.includes("output")
      && (message.includes("exist") || message.includes("clobber"))
  })
  if (outputCollision || (normalized.includes("output") && (normalized.includes("exist") || normalized.includes("clobber"))))
    return "The output already exists and will not be overwritten. Entering a different output path requires creating a new task."
  const outsideRoots = error.fieldErrors.some((fieldError) => {
    const message = fieldError.message.toLocaleLowerCase()
    return (fieldError.field === "input_path" || fieldError.field === "output_path")
      && fieldError.code === "invalid_value"
      && (message.includes("root") || message.includes("outside"))
  })
  if (outsideRoots || normalized.includes("root") || normalized.includes("outside"))
    return "The path is outside the configured roots. Enter an exact permitted path and create a new task."
  if (error.code === "conflict")
    return "The idempotency key was already used with a different request. The changed form is now a new intent and will receive a new key."
  return `${error.message} Correct the reported field and submit a new task intent.`
}
