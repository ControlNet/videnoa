import { fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

import type { Task } from "../api/taskSchemas"
import { parseTaskQuery } from "./query"
import { TaskTable } from "./TaskTable"
import { TaskToolbar } from "./TaskToolbar"

const failedTask = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  version: 3,
  status: "failed",
  input_path: "/媒体/输入/第一话.mkv",
  output_path: "/媒体/输出/第一话.mp4",
  input_extension: "mkv",
  output_extension: "mp4",
  workflow: "anime-2x",
  priority: 1,
  source: "manual",
  source_reference: null,
  input_size: 1024,
  worker_id: "550e8400-e29b-41d4-a716-446655440001",
  remote_job_id: "550e8400-e29b-41d4-a716-446655440099",
  progress: {
    percent: 100,
    processed_frames: 1000,
    total_frames: 1000,
    frames_per_second: null,
    eta_seconds: null,
    bytes_transferred: null,
    bytes_total: null,
  },
  attempt_count: 3,
  failure: {
    failure_stage: "processing",
    failure_code: "processing_failed",
    message: "處理節點回報失敗。",
    retryable: true,
  },
  cancel_requested_at: null,
  created_at: "2030-01-01T00:00:00Z",
  updated_at: "2030-01-01T00:02:00Z",
  completed_at: "2030-01-01T00:02:00Z",
} as const satisfies Task

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class {
    observe(): void {}
    disconnect(): void {}
  })
})

afterEach(() => vi.unstubAllGlobals())

describe("task Source and Failure Stage controls", () => {
  it("emits typed filter patches that reset pagination", () => {
    // Given: the task toolbar and its default URL-derived query.
    const onQueryChange = vi.fn()
    render(
      <TaskToolbar
        query={parseTaskQuery(new URLSearchParams())}
        search=""
        onQueryChange={onQueryChange}
        onSearchChange={() => undefined}
      />,
    )

    // When: the operator selects a Source and Failure Stage.
    fireEvent.change(screen.getByRole("combobox", { name: "Source" }), { target: { value: "api" } })
    fireEvent.change(screen.getByRole("combobox", { name: "Failure Stage" }), { target: { value: "publication" } })

    // Then: each server filter is emitted independently at the first page.
    expect(onQueryChange).toHaveBeenNthCalledWith(1, { source: "api", offset: 0 })
    expect(onQueryChange).toHaveBeenNthCalledWith(2, { failureStage: "publication", offset: 0 })
  })
})

describe("task optional columns", () => {
  it("renders Input Path, Output Path, and failure evidence as independent columns", () => {
    // Given: every optional column enabled through shareable URL state.
    const query = parseTaskQuery(new URLSearchParams(
      "columns=input_path,output_path,attempts,duration,failure_stage,failure,error,remote_job_id",
    ))

    // When: one failed task renders in the dense result table.
    render(
      <TaskTable
        page={{ items: [failedTask], total: 1, limit: 50, offset: 0 }}
        columns={query.columns}
        loading={false}
      />,
    )
    const table = screen.getByRole("table")
    const row = within(table).getByRole("row", { name: /第一话\.mkv/ })

    // Then: exact headers and values remain distinct, with no ambiguous Path column.
    for (const label of [
      /^Input Path$/,
      /^Output Path$/,
      /^Attempts$/,
      /^Duration$/,
      /^Failure Stage$/,
      /^Failure$/,
      /^Error$/,
      /^Remote Job ID$/,
    ]) {
      expect(within(table).getByRole("columnheader", { name: label })).toBeInTheDocument()
    }
    expect(within(table).queryByRole("columnheader", { name: /^Path$/ })).not.toBeInTheDocument()
    expect(within(row).getByText(failedTask.input_path, { exact: true })).toBeInTheDocument()
    expect(within(row).getByText(failedTask.output_path, { exact: true })).toBeInTheDocument()
    expect(within(row).getByText("processing", { exact: true })).toBeInTheDocument()
    expect(within(row).getByText("processing_failed", { exact: true })).toBeInTheDocument()
    expect(within(row).getByTitle(failedTask.failure.message)).toHaveTextContent(failedTask.failure.message)
  })
})
