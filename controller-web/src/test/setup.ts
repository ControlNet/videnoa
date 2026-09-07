import "@testing-library/jest-dom/vitest"
import { cleanup } from "@testing-library/react"
import { afterEach } from "vitest"

function showModal(this: HTMLDialogElement): void {
  this.open = true
}

function closeDialog(this: HTMLDialogElement): void {
  this.open = false
}

Object.defineProperty(HTMLDialogElement.prototype, "showModal", { configurable: true, value: showModal })
Object.defineProperty(HTMLDialogElement.prototype, "close", { configurable: true, value: closeDialog })

afterEach(cleanup)
