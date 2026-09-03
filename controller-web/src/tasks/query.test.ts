import { describe, expect, it } from "vitest"

import { canonicalLastOffset, parseTaskQuery, serializeTaskQuery } from "./query"

describe("task query state", () => {
  it("returns the canonical last-page offset for bounded results", () => {
    // Given: empty, partial-page, and multi-page result totals.
    const cases = [
      { total: 0, limit: 50, offset: 0 },
      { total: 1, limit: 50, offset: 0 },
      { total: 50, limit: 50, offset: 0 },
      { total: 51, limit: 50, offset: 50 },
      { total: 20_000, limit: 50, offset: 19_950 },
    ] as const

    // When: each total is converted to its final valid page boundary.
    const offsets = cases.map(({ total, limit }) => canonicalLastOffset(total, limit))

    // Then: correction targets the canonical page without intermediate offsets.
    expect(offsets).toEqual(cases.map(({ offset }) => offset))
  })

  it("parses supported filters and clamps invalid paging values", () => {
    // Given: a URL containing valid filters and invalid paging values.
    const parameters = new URLSearchParams(
      "status=processing&workflow=anime&worker=node-1&search=episode&sort=created_at&order=asc&limit=999&offset=-4&columns=path,remote_job",
    )

    // When: the route parses the query boundary.
    const query = parseTaskQuery(parameters)

    // Then: filters survive while paging falls back to bounded defaults.
    expect(query).toEqual({
      status: "processing",
      workflow: "anime",
      worker: "node-1",
      search: "episode",
      sort: "created_at",
      order: "asc",
      limit: 50,
      offset: 0,
      columns: ["path", "remote_job"],
    })
  })

  it("serializes only meaningful canonical state", () => {
    // Given: default filters with a non-default page and one optional column.
    const query = parseTaskQuery(new URLSearchParams("offset=50&columns=error"))

    // When: route state is serialized.
    const parameters = serializeTaskQuery(query)

    // Then: defaults are omitted and the bounded page remains shareable.
    expect(parameters.toString()).toBe("offset=50&columns=error")
  })
})
