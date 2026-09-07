import { act, fireEvent, render, screen } from "@testing-library/react"
import { useRef, useState } from "react"
import { afterEach, describe, expect, it, vi } from "vitest"

import type { ApiClient } from "../api/client"
import { ManualTaskField } from "./ManualTaskField"
import type { TaskSuggestionField } from "./taskSuggestions"

function Field({ client, name = "input_path" }: { client: ApiClient; name?: TaskSuggestionField }) {
  const [value, setValue] = useState("")
  const inputRef = useRef<HTMLInputElement>(null)
  return <ManualTaskField apiClient={client} enabled label="Path" name={name} value={value} onChange={setValue} error={undefined} inputRef={inputRef} />
}

function client(request: ApiClient["request"]): ApiClient {
  return { request, clearCsrfProof: () => undefined, csrfProof: () => null }
}

async function debounce(): Promise<void> {
  await act(async () => { await vi.advanceTimersByTimeAsync(300) })
}

describe("task field completion", () => {
  afterEach(() => vi.useRealTimers())

  it("debounces typing, navigates directories, selects files and keeps Escape inside the field", async () => {
    vi.useFakeTimers()
    const request = vi.fn().mockResolvedValueOnce({ items: [{ value: "/media/Series/", kind: "directory" }], truncated: false })
      .mockResolvedValue({ items: [{ value: "/media/Series/Episode 01.mkv", kind: "file" }], truncated: false })
    const escaped = vi.fn()
    render(<div onKeyDown={escaped}><Field client={client(request)} /></div>)
    const input = screen.getByRole("combobox")
    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: "/m" } })
    fireEvent.change(input, { target: { value: "/media/S" } })
    expect(request).not.toHaveBeenCalled()
    await debounce()
    expect(request).toHaveBeenCalledTimes(1)
    expect(request.mock.calls[0]?.[0]).toContain("prefix=%2Fmedia%2FS&kind=input")
    fireEvent.keyDown(input, { key: "ArrowDown" })
    expect(input).toHaveAttribute("aria-activedescendant", screen.getByRole("option").id)
    fireEvent.keyDown(input, { key: "Enter" })
    expect(input).toHaveValue("/media/Series/")
    await debounce()
    fireEvent.keyDown(input, { key: "ArrowUp" })
    fireEvent.keyDown(input, { key: "Enter" })
    expect(input).toHaveValue("/media/Series/Episode 01.mkv")
    expect(input).toHaveAttribute("aria-expanded", "false")
    fireEvent.keyDown(input, { key: "ArrowDown" })
    escaped.mockClear()
    fireEvent.keyDown(input, { key: "Escape" })
    expect(escaped).not.toHaveBeenCalled()
    expect(input).toHaveAttribute("aria-expanded", "false")
  })

  it("discards an old response and aborts requests when focus leaves", async () => {
    vi.useFakeTimers()
    let resolveOld: ((value: unknown) => void) | undefined
    const request = vi.fn().mockImplementationOnce(() => new Promise((resolve) => { resolveOld = resolve }))
      .mockResolvedValue({ items: [{ value: "/new/video.mkv", kind: "file" }], truncated: false })
    render(<Field client={client(request)} />)
    const input = screen.getByRole("combobox")
    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: "/old/" } })
    await debounce()
    fireEvent.change(input, { target: { value: "/new/" } })
    expect(request.mock.calls[0]?.[1].signal.aborted).toBe(true)
    await debounce()
    await act(async () => { resolveOld?.({ items: [{ value: "/old/obsolete.mkv", kind: "file" }], truncated: false }) })
    expect(screen.queryByText("obsolete.mkv")).not.toBeInTheDocument()
    expect(screen.getByRole("option")).toHaveTextContent("video.mkv")
    fireEvent.blur(input)
    expect(request.mock.calls[1]?.[1].signal.aborted).toBe(true)
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument()
  })

  it("keeps output editable after lookup failure and allows a new filename", async () => {
    vi.useFakeTimers()
    const request = vi.fn().mockRejectedValue(new Error("offline"))
    render(<Field client={client(request)} name="output_path" />)
    const input = screen.getByRole("combobox")
    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: "/media/new/result.mp4" } })
    await debounce()
    expect(screen.getByRole("status")).toHaveTextContent("Suggestions unavailable.")
    expect(input).toHaveValue("/media/new/result.mp4")
    expect(input).not.toHaveAttribute("aria-invalid")
    expect(request.mock.calls[0]?.[0]).toContain("kind=output")
  })
})
