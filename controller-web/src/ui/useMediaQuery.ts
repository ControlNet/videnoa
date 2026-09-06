import { useCallback, useSyncExternalStore } from "react"

/**
 * Subscribes to a media query.
 *
 * The shell renders one navigation, not two hidden by CSS: a duplicate set of
 * links and a duplicate sign-out would both reach the accessibility tree in any
 * environment that does not apply the stylesheet.
 */
export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback(
    (onChange: () => void) => {
      const list = window.matchMedia?.(query)
      if (list === undefined) return () => undefined
      list.addEventListener("change", onChange)
      return () => list.removeEventListener("change", onChange)
    },
    [query],
  )
  const getSnapshot = useCallback(() => window.matchMedia?.(query).matches ?? false, [query])
  return useSyncExternalStore(subscribe, getSnapshot, () => false)
}
