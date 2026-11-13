import { describe, expect, it } from "vitest"

import { appendDelta, type Delta, deltasSince, RECENT_DELTA_LIMIT } from "./deltas"

function publishAll(count: number): readonly Delta<number>[] {
  let recent: readonly Delta<number>[] = []
  for (let generation = 1; generation <= count; generation += 1) {
    recent = appendDelta(recent, { generation, value: generation * 10 })
  }
  return recent
}

describe("deltasSince", () => {
  it("returns every value published after the applied generation", () => {
    expect(deltasSince(publishAll(3), 1)).toEqual({ values: [20, 30], complete: true })
    expect(deltasSince(publishAll(3), 3)).toEqual({ values: [], complete: true })
  })

  it("reports a gap once older deltas were dropped", () => {
    const recent = publishAll(RECENT_DELTA_LIMIT + 5)
    expect(recent).toHaveLength(RECENT_DELTA_LIMIT)
    expect(deltasSince(recent, 2).complete).toBe(false)
    expect(deltasSince(recent, 5).complete).toBe(true)
  })
})
