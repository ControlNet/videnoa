import { render, screen } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { App } from "./App"

describe("Controller shell", () => {
  it("shows the operational boundary when the SPA loads", () => {
    // Given: the Controller application shell.
    render(<App />)

    // When: the initial route renders.
    const heading = screen.getByRole("heading", { name: "Controller workspace" })

    // Then: the product identity and live service contract are visible.
    expect(heading).toBeVisible()
    expect(screen.getByText("Videnoa Controller")).toBeVisible()
    expect(screen.getByText("Service online")).toBeVisible()
    expect(screen.getByText("/api/health")).toBeVisible()
  })
})
