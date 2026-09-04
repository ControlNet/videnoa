import { expect, type Locator } from "@playwright/test"

type ControlStyle = {
  readonly backgroundColor: string
  readonly borderColor: string
  readonly color: string
  readonly cursor: string
  readonly opacity: string
}

export async function expectUnavailableControlStyle(unavailable: Locator, available: Locator): Promise<void> {
  await expect(unavailable).toBeDisabled()
  await expect(available).toBeEnabled()

  const [unavailableStyle, availableStyle] = await Promise.all([
    computedControlStyle(unavailable),
    computedControlStyle(available),
  ])

  expect(unavailableStyle.cursor).toBe("not-allowed")
  expect(availableStyle.cursor).toBe("pointer")
  expect(unavailableStyle.color).not.toBe(availableStyle.color)
  expect(unavailableStyle.backgroundColor).not.toBe(availableStyle.backgroundColor)
  expect(unavailableStyle.borderColor).not.toBe(availableStyle.borderColor)
  expect(unavailableStyle.opacity).not.toBe(availableStyle.opacity)
}

async function computedControlStyle(control: Locator): Promise<ControlStyle> {
  return control.evaluate((element) => {
    const style = getComputedStyle(element)
    return {
      backgroundColor: style.backgroundColor,
      borderColor: style.borderColor,
      color: style.color,
      cursor: style.cursor,
      opacity: style.opacity,
    }
  })
}
