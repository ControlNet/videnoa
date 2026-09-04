import { act, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { parseTaskQuery } from "./query"
import { TaskCounters } from "./TaskCounters"
import { TaskTable } from "./TaskTable"
import { TaskToolbar } from "./TaskToolbar"

describe("task surface accessibility", () => {
  afterEach(() => vi.unstubAllGlobals())

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

  it("exposes measured overflow navigation and updates keyboard boundaries after resize", () => {
    // Given: a loading task table whose frame changes between overflowing and fitting.
    let notifyResize: ResizeObserverCallback | null = null
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: ResizeObserverCallback) { notifyResize = callback }
      observe(): void {}
      disconnect(): void {}
    })
    render(<TaskTable page={null} columns={[]} loading />)
    const frame = screen.getByRole("region", { name: "Scrollable task results" })
    const table = screen.getByRole("table")
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 810 })
    setScrollGeometry(table, { clientWidth: 800, scrollWidth: 800, offsetWidth: 800 })
    Object.defineProperty(frame, "scrollBy", { configurable: true, value: ({ left = 0 }: ScrollToOptions) => {
      frame.scrollLeft = Math.max(0, Math.min(frame.scrollWidth - frame.clientWidth, frame.scrollLeft + left))
      fireEvent.scroll(frame)
    } })

    // When: measured overflow is reported and the keyboard pans the focused frame.
    act(() => notifyResize?.([], {} as ResizeObserver))
    const left = screen.getByRole("button", { name: "Scroll task table left" })
    const right = screen.getByRole("button", { name: "Scroll task table right" })
    frame.focus()
    fireEvent.keyDown(frame, { key: "ArrowRight" })

    // Then: the region is described, focusable, scrollable, and boundary-aware.
    expect(frame).toHaveAccessibleDescription("Use the arrow keys or table navigation controls to view hidden columns.")
    expect(frame).toHaveAttribute("tabindex", "0")
    expect(frame.scrollLeft).toBeGreaterThan(0)
    expect(left).not.toBeDisabled()
    expect(right).not.toBeDisabled()

    // When: Home and ArrowLeft return toward the left, then End reaches the right boundary.
    fireEvent.keyDown(frame, { key: "Home" })
    expect(frame.scrollLeft).toBe(0)
    expect(left).toBeDisabled()
    fireEvent.keyDown(frame, { key: "End" })
    fireEvent.keyDown(frame, { key: "ArrowLeft" })
    expect(frame.scrollLeft).toBeLessThan(frame.scrollWidth - frame.clientWidth)
    expect(right).not.toBeDisabled()
    fireEvent.keyDown(frame, { key: "End" })
    expect(right).toBeDisabled()

    // When: late layout growth extends the scroll range after reaching the right edge.
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 820 })
    setScrollGeometry(table, { clientWidth: 810, scrollWidth: 810, offsetWidth: 810 })
    fireEvent.scroll(frame)
    act(() => notifyResize?.([], {} as ResizeObserver))

    // Then: a scroll notification delivered before resize cannot clear explicit edge intent.
    expect(frame.scrollLeft).toBe(420)
    expect(right).toBeDisabled()

    // When: the user moves left while another layout change extends the range.
    frame.scrollLeft = 419.5
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 830 })
    setScrollGeometry(table, { clientWidth: 820, scrollWidth: 820, offsetWidth: 820 })
    fireEvent.scroll(frame)
    act(() => notifyResize?.([], {} as ResizeObserver))

    // Then: even sub-tolerance user movement clears sticky edge intent before anchoring.
    expect(frame.scrollLeft).toBe(419.5)
    expect(right).not.toBeDisabled()

    // When: the resized table no longer overflows.
    setScrollGeometry(frame, { clientWidth: 800, scrollWidth: 810 })
    setScrollGeometry(table, { clientWidth: 800, scrollWidth: 800, offsetWidth: 800 })
    act(() => notifyResize?.([], {} as ResizeObserver))

    // Then: stale controls and keyboard-only affordances are removed without reserving space.
    expect(screen.queryByRole("navigation", { name: "Task table horizontal navigation" })).not.toBeInTheDocument()
    expect(frame).toHaveAttribute("tabindex", "-1")
    expect(frame).not.toHaveAttribute("aria-describedby")
  })

  it("exposes navigation for a one-pixel content overflow", () => {
    // Given: reserved scrollbar space plus one pixel of overflowing table content.
    let notifyResize: ResizeObserverCallback | null = null
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: ResizeObserverCallback) { notifyResize = callback }
      observe(): void {}
      disconnect(): void {}
    })
    render(<TaskTable page={null} columns={[]} loading />)
    const frame = screen.getByRole("region", { name: "Scrollable task results" })
    const table = screen.getByRole("table")
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 411 })
    setScrollGeometry(table, { clientWidth: 401, scrollWidth: 401, offsetWidth: 401 })

    // When: the table measurement reports the smallest integer overflow.
    act(() => notifyResize?.([], {} as ResizeObserver))

    // Then: navigation remains discoverable despite the frame's reserved gutter.
    expect(screen.getByRole("navigation", { name: "Task table horizontal navigation" })).toBeInTheDocument()
    expect(frame).toHaveAttribute("tabindex", "0")
  })

  it("does not infer right-edge intent from initial layout growth", () => {
    // Given: an overflowing table that has never been horizontally navigated.
    let notifyResize: ResizeObserverCallback | null = null
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: ResizeObserverCallback) { notifyResize = callback }
      observe(): void {}
      disconnect(): void {}
    })
    render(<TaskTable page={null} columns={[]} loading />)
    const frame = screen.getByRole("region", { name: "Scrollable task results" })
    const table = screen.getByRole("table")
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 410 })
    setScrollGeometry(table, { clientWidth: 401, scrollWidth: 401, offsetWidth: 401 })
    act(() => notifyResize?.([], {} as ResizeObserver))

    // When: layout growth extends the range without user navigation.
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 420 })
    setScrollGeometry(table, { clientWidth: 411, scrollWidth: 411, offsetWidth: 411 })
    fireEvent.scroll(frame)
    act(() => notifyResize?.([], {} as ResizeObserver))

    // Then: the table remains at its initial left edge.
    expect(frame.scrollLeft).toBe(0)
  })

  it("preserves explicit End intent from the smallest effective range", () => {
    // Given: one pixel of table overflow inside a ten-pixel stable scrollbar gutter.
    let notifyResize: ResizeObserverCallback | null = null
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: ResizeObserverCallback) { notifyResize = callback }
      observe(): void {}
      disconnect(): void {}
    })
    render(<TaskTable page={null} columns={[]} loading />)
    const frame = screen.getByRole("region", { name: "Scrollable task results" })
    const table = screen.getByRole("table")
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 410, offsetWidth: 410 })
    setScrollGeometry(table, { clientWidth: 401, scrollWidth: 401, offsetWidth: 401 })
    act(() => notifyResize?.([], {} as ResizeObserver))
    frame.focus()
    fireEvent.keyDown(frame, { key: "End" })

    // When: later layout growth extends the effective range.
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 420, offsetWidth: 410 })
    setScrollGeometry(table, { clientWidth: 411, scrollWidth: 411, offsetWidth: 411 })
    fireEvent.scroll(frame)
    act(() => notifyResize?.([], {} as ResizeObserver))

    // Then: explicit keyboard intent remains anchored at the new edge.
    expect(frame.scrollLeft).toBe(20)
  })

  it("preserves explicit End intent issued before task rows create overflow", () => {
    // Given: the loading table exists, but its task rows have not created overflow yet.
    let notifyResize: ResizeObserverCallback | null = null
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: ResizeObserverCallback) { notifyResize = callback }
      observe(): void {}
      disconnect(): void {}
    })
    render(<TaskTable page={null} columns={[]} loading />)
    const frame = screen.getByRole("region", { name: "Scrollable task results" })
    const table = screen.getByRole("table")
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 400 })
    setScrollGeometry(table, { clientWidth: 400, scrollWidth: 400, offsetWidth: 400 })
    act(() => notifyResize?.([], {} as ResizeObserver))
    frame.focus()

    // When: End is pressed before asynchronous task rows expand the table.
    fireEvent.keyDown(frame, { key: "End" })
    expect(screen.queryByRole("navigation", { name: "Task table horizontal navigation" })).not.toBeInTheDocument()
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 820 })
    setScrollGeometry(table, { clientWidth: 820, scrollWidth: 820, offsetWidth: 820 })
    act(() => notifyResize?.([], {} as ResizeObserver))

    // Then: the committed intent reaches the new edge without exposing two enabled controls.
    expect(frame.scrollLeft).toBe(420)
    expect(screen.getByRole("button", { name: "Scroll task table left" })).not.toBeDisabled()
    expect(screen.getByRole("button", { name: "Scroll task table right" })).toBeDisabled()
  })

  it("preserves explicit End intent through initial edge contraction and restoration", () => {
    // Given: End has reached the right edge while initial layout observers are still active.
    let notifyResize: ResizeObserverCallback | null = null
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: ResizeObserverCallback) { notifyResize = callback }
      observe(): void {}
      disconnect(): void {}
    })
    render(<TaskTable page={null} columns={[]} loading />)
    const frame = screen.getByRole("region", { name: "Scrollable task results" })
    const table = screen.getByRole("table")
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 820 })
    setScrollGeometry(table, { clientWidth: 820, scrollWidth: 820, offsetWidth: 820 })
    act(() => notifyResize?.([], {} as ResizeObserver))
    frame.focus()
    fireEvent.keyDown(frame, { key: "End" })
    const left = screen.getByRole("button", { name: "Scroll task table left" })
    const right = screen.getByRole("button", { name: "Scroll task table right" })
    expect(right).toBeDisabled()

    // When: initial layout contracts the range and clamps the edge, then restores it.
    frame.scrollLeft = 416
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 816 })
    setScrollGeometry(table, { clientWidth: 816, scrollWidth: 816, offsetWidth: 816 })
    fireEvent.scroll(frame)
    act(() => notifyResize?.([], {} as ResizeObserver))
    setScrollGeometry(frame, { clientWidth: 400, scrollWidth: 820 })
    setScrollGeometry(table, { clientWidth: 820, scrollWidth: 820, offsetWidth: 820 })
    act(() => notifyResize?.([], {} as ResizeObserver))

    // Then: observer-owned geometry activity keeps the frame at the rendered edge.
    expect(frame.scrollLeft).toBe(420)
    expect(left).not.toBeDisabled()
    expect(right).toBeDisabled()
  })
})

function setScrollGeometry(element: HTMLElement, dimensions: { readonly clientWidth: number; readonly scrollWidth: number; readonly offsetWidth?: number }): void {
  Object.defineProperties(element, {
    clientWidth: { configurable: true, value: dimensions.clientWidth },
    scrollWidth: { configurable: true, value: dimensions.scrollWidth },
    offsetWidth: { configurable: true, value: dimensions.offsetWidth ?? dimensions.clientWidth },
  })
}
