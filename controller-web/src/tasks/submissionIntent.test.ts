import { describe, expect, it, vi } from "vitest"

import type { TaskCreateRequest } from "../api/taskSchemas"
import { beginSubmission, markSubmissionAmbiguous } from "./submissionIntent"

const request = {
  input_path: "/nas/input/exact.mkv",
  output_path: "/nas/output/exact.mp4",
  workflow: "anime-2x",
  priority: 17,
  source: "manual",
  source_reference: null,
} as const satisfies TaskCreateRequest

describe("manual task submission intent", () => {
  it("reuses one UUID after a dropped response for the identical body", () => {
    // Given: one generated intent whose response became ambiguous.
    const randomUUID = vi.fn().mockReturnValue("550e8400-e29b-41d4-a716-446655440001")
    const first = beginSubmission(null, request, randomUUID)
    const ambiguous = markSubmissionAmbiguous(first)

    // When: the operator retries the unchanged request.
    const replay = beginSubmission(ambiguous, { ...request }, randomUUID)

    // Then: one logical intent keeps one key.
    expect(replay.key).toBe(first.key)
    expect(randomUUID).toHaveBeenCalledOnce()
  })

  it("generates a new UUID when any body field changes after ambiguity", () => {
    // Given: an ambiguous intent and a second UUID source value.
    const randomUUID = vi.fn().mockReturnValueOnce("550e8400-e29b-41d4-a716-446655440001").mockReturnValueOnce("550e8400-e29b-41d4-a716-446655440002")
    const ambiguous = markSubmissionAmbiguous(beginSubmission(null, request, randomUUID))

    // When: the exact output path changes.
    const changed = beginSubmission(ambiguous, { ...request, output_path: "/nas/output/new.mp4" }, randomUUID)

    // Then: the changed intent cannot collide with the earlier body.
    expect(changed.key).not.toBe(ambiguous.key)
    expect(randomUUID).toHaveBeenCalledTimes(2)
  })
})
