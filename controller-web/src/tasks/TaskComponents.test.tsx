import { render, screen } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { parseTaskQuery } from "./query"
import { TaskCounters } from "./TaskCounters"
import { TaskTable } from "./TaskTable"
import { TaskToolbar } from "./TaskToolbar"

describe("task surface accessibility", () => {
  it("provides complete metadata for path and identifier filters", () => {
    // Given: the task toolbar with its default URL query.
    render(
      <TaskToolbar
        query={parseTaskQuery(new URLSearchParams())}
        search=""
        onQueryChange={() => undefined}
        onSearchChange={() => undefined}
      />,
    )

    // When: assistive and browser form metadata is inspected.
    const search = screen.getByLabelText("Search task paths")
    const workflow = screen.getByLabelText("Workflow")
    const worker = screen.getByLabelText("Worker ID")

    // Then: every text filter is named, avoids autofill, and disables harmful correction.
    for (const [input, name] of [[search, "search"], [workflow, "workflow"], [worker, "worker"]] as const) {
      expect(input).toHaveAttribute("name", name)
      expect(input).toHaveAttribute("autocomplete", "off")
      expect(input).toHaveAttribute("spellcheck", "false")
    }
  })

  it("announces counter and empty-result changes politely", () => {
    // Given: unloaded counters and an empty successful task page.
    render(
      <>
        <TaskCounters counts={null} />
        <TaskTable page={{ items: [], total: 0, limit: 50, offset: 0 }} columns={[]} loading={false} />
      </>,
    )

    // When: the dynamic status regions are inspected.
    const counters = screen.getByLabelText("Task status counts")
    const empty = screen.getByRole("status")

    // Then: meaningful asynchronous changes use restrained polite announcements.
    expect(counters).toHaveAttribute("aria-live", "polite")
    expect(counters).toHaveAttribute("aria-atomic", "true")
    expect(empty).toHaveAttribute("aria-live", "polite")
    expect(empty).toHaveTextContent("No tasks match this view.")
  })

  it("exposes the scrollable task results and navigation to keyboard users", () => {
    // Given: a loading task table with overflow owned by its frame.
    render(<TaskTable page={null} columns={[]} loading />)

    // When: the overflow owner and its horizontal controls are reached by name.
    const frame = screen.getByRole("region", { name: "Scrollable task results" })

    // Then: the named region has explicit keyboard-operable navigation.
    expect(frame).toHaveAccessibleDescription("Scroll table to view more columns.")
    expect(screen.getByRole("button", { name: "Scroll task table left" })).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Scroll task table right" })).toBeInTheDocument()
  })
})
