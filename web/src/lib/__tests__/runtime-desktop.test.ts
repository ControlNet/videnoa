import { describe, expect, it, vi } from 'vitest'
import { createDesktopWindowController, isDesktopRuntime } from '../runtime-desktop'

describe('isDesktopRuntime()', () => {
  it('returns false in browser-like runtime without Tauri globals', () => {
    expect(isDesktopRuntime({})).toBe(false)
  })

  it('returns true when Tauri v2 current window API is available', () => {
    const runtime = {
      __TAURI__: {
        window: {
          getCurrentWindow: () => ({ minimize: vi.fn() }),
        },
      },
    }

    expect(isDesktopRuntime(runtime)).toBe(true)
  })

  it('returns true when Tauri v1 appWindow API is available', () => {
    const runtime = {
      __TAURI__: {
        window: {
          appWindow: { minimize: vi.fn() },
        },
      },
    }

    expect(isDesktopRuntime(runtime)).toBe(true)
  })
})

describe('createDesktopWindowController()', () => {
  it('no-ops safely in browser runtime', async () => {
    const controller = createDesktopWindowController({})

    expect(controller.isDesktop).toBe(false)
    await expect(controller.minimize()).resolves.toBe(false)
    await expect(controller.toggleMaximize()).resolves.toBe(false)
    await expect(controller.close()).resolves.toBe(false)
  })

  it('invokes window controls in desktop runtime', async () => {
    const minimize = vi.fn()
    const toggleMaximize = vi.fn()
    const close = vi.fn()

    const runtime = {
      __TAURI__: {
        window: {
          getCurrentWindow: () => ({ minimize, toggleMaximize, close }),
        },
      },
    }

    const controller = createDesktopWindowController(runtime)

    expect(controller.isDesktop).toBe(true)
    await expect(controller.minimize()).resolves.toBe(true)
    await expect(controller.toggleMaximize()).resolves.toBe(true)
    await expect(controller.close()).resolves.toBe(true)
    expect(minimize).toHaveBeenCalledTimes(1)
    expect(toggleMaximize).toHaveBeenCalledTimes(1)
    expect(close).toHaveBeenCalledTimes(1)
  })

  it('binds window API method context for controls', async () => {
    const contextMarkers: string[] = []
    const windowApi = {
      marker: 'desktop-window-api',
      minimize(this: { marker: string }) {
        contextMarkers.push(this.marker)
      },
      toggleMaximize(this: { marker: string }) {
        contextMarkers.push(this.marker)
      },
      close(this: { marker: string }) {
        contextMarkers.push(this.marker)
      },
    }

    const runtime = {
      __TAURI__: {
        window: {
          getCurrentWindow: () => windowApi,
        },
      },
    }

    const controller = createDesktopWindowController(runtime)

    await expect(controller.minimize()).resolves.toBe(true)
    await expect(controller.toggleMaximize()).resolves.toBe(true)
    await expect(controller.close()).resolves.toBe(true)
    expect(contextMarkers).toEqual([
      'desktop-window-api',
      'desktop-window-api',
      'desktop-window-api',
    ])
  })
})
