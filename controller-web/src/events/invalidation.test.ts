import { describe, expect, it, vi } from "vitest"

import { createInvalidationStore } from "./invalidation"

describe("application invalidation", () => {
  it("notifies active route consumers for initial, reconnect, and lag refetch signals", () => {
    // Given: a route consumer subscribed to shell invalidation.
    const store = createInvalidationStore()
    const subscriber = vi.fn()
    store.subscribe(subscriber)

    // When: SSE reports snapshot-required signals without event history.
    store.invalidate("initial")
    store.invalidate("reconnect")
    store.invalidate("lag")

    // Then: every signal advances the typed snapshot generation.
    expect(subscriber).toHaveBeenCalledTimes(3)
    expect(store.snapshot()).toEqual({ generation: 3, reason: "lag" })
  })
})
