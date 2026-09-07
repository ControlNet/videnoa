import { describe, expect, it } from "vitest"

import { batchRowErrors, type BatchRow } from "./batchTask"
import { beginSubmission } from "./submissionIntent"

// Synthetic preview records only; no filesystem or Worker is accessed.
function row(input: string, patch: Partial<BatchRow> = {}): BatchRow {
  const request = { input_path: input, output_path: "/output/same.mkv", workflow: "anime", priority: 0, source: "manual", source_reference: null } as const
  return { request, error: "Multiple inputs map to this output path.", validation_error: null,
    output_key: "/output/same.mkv", excluded: false, status: "ready", submissionError: null,
    intent: beginSubmission(null, request, () => input), ...patch }
}

describe("selected batch conflicts", () => {
  it("recalculates duplicate destinations when a row is removed and restored", () => {
    const first = row("/a/video.mkv")
    const second = row("/b/video.mkv")
    expect(batchRowErrors([first, second]).get(first.request.input_path)).toContain("Multiple inputs")
    expect([...batchRowErrors([first, { ...second, excluded: true }]).values()]).toEqual([null, null])
    expect(batchRowErrors([first, second]).get(second.request.input_path)).toContain("Multiple inputs")
  })

  it("keeps independent path errors after a duplicate is removed", () => {
    const first = row("/a/video.mkv", { validation_error: "Output already exists." })
    const second = row("/b/video.mkv", { excluded: true })
    expect(batchRowErrors([first, second]).get(first.request.input_path)).toBe("Output already exists.")
    expect(batchRowErrors([{ ...first, excluded: true }, second]).get(first.request.input_path)).toBeNull()
  })

  it("uses the server comparison key even when path spelling differs", () => {
    const first = row("/a/video.mkv")
    const second = row("/b/video.mkv", { request: { ...first.request, input_path: "/b/video.mkv", output_path: "/OUTPUT/SAME.MKV" } })
    expect(batchRowErrors([first, second]).get(second.request.input_path)).toContain("Multiple inputs")
  })

  it("preserves unclassified errors returned by older servers", () => {
    const first = row("/a/video.mkv")
    delete first.validation_error
    expect(batchRowErrors([first]).get(first.request.input_path)).toContain("Multiple inputs")
  })
})
