type WindowControlMethod = 'minimize' | 'toggleMaximize' | 'close'

type WindowControlFunction = (this: DesktopWindowApi) => Promise<void> | void

interface DesktopWindowApi {
  minimize?: WindowControlFunction
  toggleMaximize?: WindowControlFunction
  close?: WindowControlFunction
}

interface DesktopGlobalLike {
  __TAURI__?: {
    window?: {
      getCurrentWindow?: () => DesktopWindowApi
      appWindow?: DesktopWindowApi
    }
  }
}

function resolveDesktopWindowApi(source: unknown): DesktopWindowApi | null {
  const runtime = source as DesktopGlobalLike
  const windowApi = runtime.__TAURI__?.window
  if (!windowApi) return null

  const currentWindow = windowApi.getCurrentWindow?.()
  if (currentWindow) return currentWindow

  return windowApi.appWindow ?? null
}

export function isDesktopRuntime(source: unknown = globalThis): boolean {
  return resolveDesktopWindowApi(source) !== null
}

async function invokeWindowControl(
  method: WindowControlMethod,
  source: unknown = globalThis,
): Promise<boolean> {
  const windowApi = resolveDesktopWindowApi(source)
  if (!windowApi) return false

  const control = windowApi[method]
  if (typeof control !== 'function') return false

  try {
    await control.call(windowApi)
    return true
  } catch (error) {
    console.error(`Failed to invoke desktop window control: ${method}`, error)
    return false
  }
}

export function createDesktopWindowController(source: unknown = globalThis) {
  return {
    isDesktop: isDesktopRuntime(source),
    minimize: () => invokeWindowControl('minimize', source),
    toggleMaximize: () => invokeWindowControl('toggleMaximize', source),
    close: () => invokeWindowControl('close', source),
  }
}
