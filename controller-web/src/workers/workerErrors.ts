import type { ApiClientError } from "../api/client"

export function workerActionMessage(error: ApiClientError): string {
  if (error.message === "worker changed since it was read") return "Worker changed on the Controller. The current row was reloaded; review it before retrying."
  if (error.message === "worker is referenced by tasks") return "This worker is still referenced by durable tasks and cannot be deleted. Disable it instead."
  if (error.message === "worker capacity is below durable usage") return "Compute slots cannot be reduced below currently used durable capacity."
  if (error.message === "worker name is already registered") return "That worker name is already registered. Enter a unique name."
  if (error.message === "worker API URL is already registered") return "That worker API URL is already registered. Enter a unique base URL."
  return error.message
}

export function workerServerFieldErrors(error: ApiClientError | null): Partial<Record<"name" | "apiUrl" | "computeSlots", string>> {
  if (error === null) return {}
  const result: Partial<Record<"name" | "apiUrl" | "computeSlots", string>> = {}
  for (const fieldError of error.fieldErrors) {
    if (fieldError.field === "name") result.name = fieldError.message
    if (fieldError.field === "api_url") result.apiUrl = fieldError.message
    if (fieldError.field === "compute_slots") result.computeSlots = fieldError.message
  }
  if (error.message.includes("name is already")) result.name = "Enter a unique worker name."
  if (error.message.includes("API URL is already")) result.apiUrl = "Enter a unique worker API URL."
  return result
}
