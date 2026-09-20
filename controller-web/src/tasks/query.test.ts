import { describe, expect, it, vi } from "vitest"

import {
  canonicalLastOffset,
  loadTaskQuery,
  parseTaskQuery,
  persistTaskQuery,
  taskPagePath,
  taskViewStorageKey,
} from "./query"

describe("task query state", () => {
  it("restores durable view preferences without restoring transient search and paging", () => {
    // Given: a previously saved browser-local Tasks view.
    const storage = {
      getItem: vi.fn(() => JSON.stringify({
        status: "processing",
        source: "api",
        failureStage: "publication",
        workflow: "anime",
        worker: "node-1",
        sort: "created_at",
        order: "asc",
        limit: 100,
        columns: ["priority", "error"],
        search: "must-not-return",
        offset: 50,
      })),
    }

    // When: the Tasks page initializes from local storage.
    const query = loadTaskQuery(storage)

    // Then: durable controls return while session-only state starts clean.
    expect(storage.getItem).toHaveBeenCalledWith(taskViewStorageKey)
    expect(query).toEqual({
      status: "processing",
      source: "api",
      failureStage: "publication",
      workflow: "anime",
      worker: "node-1",
      search: "",
      sort: "created_at",
      order: "asc",
      limit: 100,
      offset: 0,
      columns: ["priority", "error"],
    })
  })

  it("persists view preferences without writing transient search and paging", () => {
    // Given: a Tasks query containing both view preferences and transient state.
    const storage = { setItem: vi.fn() }
    const query = parseTaskQuery(new URLSearchParams(
      "status=failed&search=episode&sort=created_at&order=asc&limit=25&offset=50&columns=priority,error",
    ))

    // When: the browser-local view is saved.
    persistTaskQuery(storage, query)

    // Then: only durable controls are stored.
    expect(storage.setItem).toHaveBeenCalledOnce()
    const call = storage.setItem.mock.calls[0]
    expect(call).toBeDefined()
    const key = call?.[0]
    const value = call?.[1]
    expect(key).toBe(taskViewStorageKey)
    expect(value).toBeTypeOf("string")
    if (typeof value !== "string") throw new Error("stored task view must be serialized")
    expect(JSON.parse(value)).toEqual({
      status: "failed",
      source: "all",
      failureStage: "all",
      workflow: "",
      worker: "",
      sort: "created_at",
      order: "asc",
      limit: 25,
      columns: ["priority", "error"],
    })
  })

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
      "status=processing&source=api&failure_stage=publication&workflow=anime&worker=node-1&search=episode&sort=created_at&order=asc&limit=999&offset=-4&columns=priority,input_path,output_path,failure_stage,remote_job_id",
    )

    // When: the route parses the query boundary.
    const query = parseTaskQuery(parameters)

    // Then: filters survive while paging falls back to bounded defaults.
    expect(query).toEqual({
      status: "processing",
      source: "api",
      failureStage: "publication",
      workflow: "anime",
      worker: "node-1",
      search: "episode",
      sort: "created_at",
      order: "asc",
      limit: 50,
      offset: 0,
      columns: ["priority", "input_path", "output_path", "failure_stage", "remote_job_id"],
    })
  })

  it("forwards Source and Failure Stage filters to the bounded task endpoint", () => {
    // Given: shareable route state with both server-backed filters selected.
    const query = parseTaskQuery(new URLSearchParams("source=manual&failure_stage=processing&limit=25&offset=50"))

    // When: the task request path is built.
    const path = taskPagePath(query)

    // Then: the API receives the exact filter names and canonical paging state.
    expect(Object.fromEntries(new URL(path, "http://controller.local/").searchParams)).toEqual({
      limit: "25",
      offset: "50",
      sort: "priority",
      direction: "desc",
      source: "manual",
      failure_stage: "processing",
    })
  })

})
