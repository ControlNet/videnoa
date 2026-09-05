import { describe, expect, it } from "vitest"

import { ApiClientError } from "../api/client"
import { manualTaskErrorMessage, manualTaskFieldErrors } from "./manualTaskForm"

describe("manual task server guidance", () => {
  it("derives workspace guidance from structured field evidence", () => {
    // Given: the generic top-level message and precise synthetic test-only workspace error.
    const error = new ApiClientError("invalid_request", 400, "request validation failed", false, [
      { field: "input_path", code: "invalid_value", message: "path is outside the task workspace" },
    ])

    // When: operator guidance is derived.
    const message = manualTaskErrorMessage(error)

    // Then: recovery uses the workspace model without exposing fixed media roots.
    expect(message).toContain("outside the Controller workspace")
    expect(message).not.toContain("configured roots")
  })

  it("derives no-clobber guidance from structured field evidence", () => {
    // Given: Rust's generic top-level message and precise output collision error.
    const error = new ApiClientError("invalid_request", 400, "request validation failed", false, [
      { field: "output_path", code: "invalid_value", message: "output must not already exist" },
    ])

    // When: operator guidance is derived.
    const message = manualTaskErrorMessage(error)

    // Then: overwrite remains forbidden and path changes require a new task.
    expect(message).toContain("will not be overwritten")
    expect(message).toContain("creating a new task")
  })

  it("preserves adjacent structured field messages", () => {
    // Given: multiple server field failures in authoritative order.
    const error = new ApiClientError("invalid_request", 400, "request validation failed", false, [
      { field: "input_path", code: "invalid_value", message: "input path failure" },
      { field: "output_path", code: "conflict", message: "output path failure" },
    ])

    // When: field messages are mapped to form controls.
    const fields = manualTaskFieldErrors(error)

    // Then: both adjacent messages survive unchanged.
    expect(fields).toEqual({ inputPath: "input path failure", outputPath: "output path failure" })
  })
})
