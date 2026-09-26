import { act, cleanup, render } from "@testing-library/react"
import { afterEach, beforeEach, expect, it, vi } from "vitest"

import { SessionEvents } from "./SessionEvents"
import { appInvalidationStore } from "./store"

// Test-only EventSource substitute: explicitly control transport lifecycle events.
class TestEventSource extends EventTarget {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2
  static instances: TestEventSource[] = []
  readonly url: string
  readonly options: EventSourceInit
  readyState = TestEventSource.CONNECTING
  close = vi.fn(() => { this.readyState = TestEventSource.CLOSED })

  constructor(url: string, options: EventSourceInit) {
    super()
    this.url = url
    this.options = options
    TestEventSource.instances.push(this)
  }

  emit(type: string, state = this.readyState) {
    this.readyState = state
    act(() => { this.dispatchEvent(new Event(type)) })
  }
}

function latest() {
  const source = TestEventSource.instances.at(-1)
  if (source === undefined) throw new Error("No test event stream was created")
  return source
}

beforeEach(() => {
  vi.useFakeTimers()
  TestEventSource.instances = []
  vi.stubGlobal("EventSource", TestEventSource)
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible")
  vi.spyOn(navigator, "onLine", "get").mockReturnValue(true)
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

it("retries a closed stream with bounded backoff and resets after recovery", () => {
  const state = vi.fn()
  render(<SessionEvents onConnectionStateChange={state} />)
  latest().emit("error", TestEventSource.CLOSED)
  expect(state).toHaveBeenLastCalledWith("unavailable")
  for (const delay of [1000, 2000, 4000, 8000, 16000, 30000, 30000]) {
    const count = TestEventSource.instances.length
    act(() => vi.advanceTimersByTime(delay - 1))
    expect(TestEventSource.instances).toHaveLength(count)
    act(() => vi.advanceTimersByTime(1))
    expect(TestEventSource.instances).toHaveLength(count + 1)
    expect(state).toHaveBeenLastCalledWith("reconnecting")
    latest().emit("error", TestEventSource.CLOSED)
  }
  act(() => window.dispatchEvent(new Event("focus")))
  latest().emit("open", TestEventSource.OPEN)
  expect(state).toHaveBeenLastCalledWith("connected")
  latest().emit("error", TestEventSource.CLOSED)
  const count = TestEventSource.instances.length
  act(() => vi.advanceTimersByTime(1000))
  expect(TestEventSource.instances).toHaveLength(count + 1)
})

it.each(["focus", "online", "visibilitychange", "pageshow"])("recovers a closed stream on %s and refreshes its snapshot", (type) => {
  const state = vi.fn()
  render(<SessionEvents onConnectionStateChange={state} />)
  const old = latest()
  old.emit("refetch", TestEventSource.OPEN)
  old.emit("error", TestEventSource.CLOSED)
  act(() => (type === "visibilitychange" ? document : window).dispatchEvent(new Event(type)))
  expect(TestEventSource.instances).toHaveLength(2)
  expect(old.close).toHaveBeenCalled()
  expect(latest().url).toBe("/api/events")
  expect(latest().options.withCredentials).toBe(true)
  const generation = appInvalidationStore.snapshot().generation
  latest().emit("refetch", TestEventSource.OPEN)
  expect(appInvalidationStore.snapshot().generation).toBe(generation + 1)
  expect(state).toHaveBeenLastCalledWith("connected")
  old.emit("error", TestEventSource.CLOSED)
  old.emit("refetch", TestEventSource.OPEN)
  expect(state).toHaveBeenLastCalledWith("connected")
  expect(appInvalidationStore.snapshot().generation).toBe(generation + 1)
  act(() => vi.advanceTimersByTime(60000))
  expect(TestEventSource.instances).toHaveLength(2)
})

it("leaves native reconnects and healthy focus events alone", () => {
  render(<SessionEvents onConnectionStateChange={vi.fn()} />)
  latest().emit("open", TestEventSource.OPEN)
  act(() => window.dispatchEvent(new Event("focus")))
  expect(TestEventSource.instances).toHaveLength(1)
  latest().emit("error", TestEventSource.CONNECTING)
  act(() => vi.advanceTimersByTime(60000))
  expect(TestEventSource.instances).toHaveLength(1)
  act(() => window.dispatchEvent(new Event("focus")))
  expect(TestEventSource.instances).toHaveLength(2)
  act(() => window.dispatchEvent(new Event("online")))
  expect(TestEventSource.instances).toHaveLength(2)
})

it.each(["hidden", "offline"])("defers closed-stream retries while %s and resumes when active", (mode) => {
  render(<SessionEvents onConnectionStateChange={vi.fn()} />)
  if (mode === "hidden") vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden")
  else vi.spyOn(navigator, "onLine", "get").mockReturnValue(false)
  latest().emit("error", TestEventSource.CLOSED)
  act(() => vi.advanceTimersByTime(60000))
  act(() => window.dispatchEvent(new Event("focus")))
  expect(TestEventSource.instances).toHaveLength(1)
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible")
  vi.spyOn(navigator, "onLine", "get").mockReturnValue(true)
  act(() => window.dispatchEvent(new Event("online")))
  expect(TestEventSource.instances).toHaveLength(2)
})

it("cleans up retries and lifecycle listeners on unmount", () => {
  const state = vi.fn()
  const { unmount } = render(<SessionEvents onConnectionStateChange={state} />)
  const old = latest()
  old.emit("error", TestEventSource.CLOSED)
  unmount()
  state.mockClear()
  act(() => {
    vi.advanceTimersByTime(60000)
    window.dispatchEvent(new Event("focus"))
    window.dispatchEvent(new Event("online"))
    window.dispatchEvent(new Event("pageshow"))
    document.dispatchEvent(new Event("visibilitychange"))
  })
  old.emit("open", TestEventSource.OPEN)
  expect(old.close).toHaveBeenCalled()
  expect(state).not.toHaveBeenCalled()
  expect(TestEventSource.instances).toHaveLength(1)
})
