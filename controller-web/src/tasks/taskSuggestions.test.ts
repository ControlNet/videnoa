import { describe, expect, it, vi } from "vitest"

import { loadTaskSuggestions } from "./taskSuggestions"

describe("workflow suggestions", () => {
  it("merges enabled worker workflows and presets, deduplicates, and filters by name", async () => {
    // Synthetic capability responses; no live Worker is contacted.
    const request = vi.fn().mockResolvedValue({ items: [
      { enabled: true, capabilities: { workflows: [{ name: "Anime 2x", kind: "workflow" }, { name: "Anime 4x", kind: "preset" }] } },
      { enabled: true, capabilities: { workflows: [{ name: "Anime 2x", kind: "workflow" }, { name: "Restore", kind: "preset" }] } },
      { enabled: false, capabilities: { workflows: [{ name: "Anime disabled", kind: "workflow" }] } },
    ] })
    const result = await loadTaskSuggestions({ request, csrfProof: () => null, clearCsrfProof: () => undefined }, "workflow", "ANIME", new AbortController().signal)
    expect(result).toEqual({ items: [{ value: "Anime 2x", kind: "workflow" }, { value: "Anime 4x", kind: "preset" }], truncated: false })
  })
})
