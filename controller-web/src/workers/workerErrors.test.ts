import { describe, expect, it } from "vitest"

import { ApiClientError } from "../api/client"
import { workerActionMessage } from "./workerErrors"

const cases = [
  ["worker changed since it was read", "current row was reloaded"],
  ["worker is referenced by tasks", "Disable it instead"],
  ["worker capacity is below durable usage", "currently used durable capacity"],
  ["worker name is already registered", "unique name"],
  ["worker API URL is already registered", "unique base URL"],
] as const

describe("worker action errors", () => {
  it.each(cases)("maps backend message %s to safe operator guidance", (message, guidance) => {
    // Given: an exact OperationsError message from the HTTP boundary.
    const error = new ApiClientError("conflict", 409, message)

    // When: the Workers surface presents the failed action.
    const result = workerActionMessage(error)

    // Then: the operator receives actionable guidance rather than raw internals.
    expect(result).toContain(guidance)
  })
})
