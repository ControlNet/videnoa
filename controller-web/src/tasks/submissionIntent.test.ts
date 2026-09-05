import { afterEach, describe, expect, it, vi } from "vitest"

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
  afterEach(() => vi.unstubAllGlobals())

  it("creates UUIDv4 keys without the secure-context-only randomUUID API", () => {
    // Given: the browser crypto surface available on a non-localhost HTTP origin.
    vi.stubGlobal("crypto", { getRandomValues: crypto.getRandomValues.bind(crypto) })

    // When: independent task submissions need keys without an injected generator.
    const keys = Array.from({ length: 32 }, () => beginSubmission(null, request).key)

    // Then: each intent has a distinct UUID with the required version and variant.
    expect(new Set(keys).size).toBe(keys.length)
    for (const key of keys) {
      expect(key).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    }
  })

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
