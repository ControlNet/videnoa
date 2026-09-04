import { expect, type Locator } from "@playwright/test"

type ControlContract = {
  readonly unavailableDisabled: boolean
  readonly availableEnabled: boolean
  readonly unavailableCursor: string
  readonly availableCursor: string
  readonly unavailableOpacity: string
  readonly opacityDiffers: boolean
  readonly colorDiffers: boolean
  readonly backgroundDiffers: boolean
  readonly borderDiffers: boolean
}

export async function expectUnavailableControlStyle(unavailable: Locator, available: Locator): Promise<void> {
  await expect.poll(() => computedControlContract(unavailable, available)).toEqual({
    unavailableDisabled: true,
    availableEnabled: true,
    unavailableCursor: "not-allowed",
    availableCursor: "pointer",
    unavailableOpacity: "0.65",
    opacityDiffers: true,
    colorDiffers: true,
    backgroundDiffers: true,
    borderDiffers: true,
  })
}

async function computedControlContract(unavailable: Locator, available: Locator): Promise<ControlContract> {
  const availableElement = await available.elementHandle()
  if (availableElement === null) throw new TypeError("available control is missing")
  try {
    return await unavailable.evaluate((unavailableElement, availableControl) => {
      if (!(unavailableElement instanceof HTMLButtonElement) || !(availableControl instanceof HTMLButtonElement)) {
        throw new TypeError("control pair must contain buttons")
      }
      const unavailableStyle = getComputedStyle(unavailableElement)
      const availableStyle = getComputedStyle(availableControl)
      return {
        unavailableDisabled: unavailableElement.disabled,
        availableEnabled: !availableControl.disabled,
        unavailableCursor: unavailableStyle.cursor,
        availableCursor: availableStyle.cursor,
        unavailableOpacity: unavailableStyle.opacity,
        opacityDiffers: unavailableStyle.opacity !== availableStyle.opacity,
        colorDiffers: unavailableStyle.color !== availableStyle.color,
        backgroundDiffers: unavailableStyle.backgroundColor !== availableStyle.backgroundColor,
        borderDiffers: unavailableStyle.borderColor !== availableStyle.borderColor,
      }
    }, availableElement)
  } finally {
    await availableElement.dispose()
  }
}
