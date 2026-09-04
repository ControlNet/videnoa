import { useCallback, useLayoutEffect, useRef, useState } from "react"

const scrollEdgeTolerance = 1

type ScrollState = { readonly hasOverflow: boolean; readonly canScrollLeft: boolean; readonly canScrollRight: boolean }
type ScrollMetrics = { readonly left: number; readonly maximum: number }
type RightEdgeIntent = "none" | "pending" | "anchored"

export type ScrollIntent = "left" | "right"
export type ScrollUpdate = (intent?: ScrollIntent) => void

const initialScrollState: ScrollState = { hasOverflow: false, canScrollLeft: false, canScrollRight: false }
const initialScrollMetrics: ScrollMetrics = { left: 0, maximum: 0 }

export function useTaskTableScroll(rendersTable: boolean) {
  const frameRef = useRef<HTMLElement>(null)
  const tableRef = useRef<HTMLTableElement>(null)
  const [scrollState, setScrollState] = useState(initialScrollState)
  const scrollMetricsRef = useRef(initialScrollMetrics)
  const rightEdgeIntentRef = useRef<RightEdgeIntent>("none")
  const updateScrollState = useCallback<ScrollUpdate>((explicitIntent) => {
    const frame = frameRef.current
    const table = tableRef.current
    if (frame === null || table === null) return
    const contentOverflow = Math.max(0, table.offsetWidth - frame.clientWidth)
    const maximumScroll = Math.max(0, frame.scrollWidth - frame.clientWidth)
    const edgeTolerance = Math.max(scrollEdgeTolerance, frame.offsetWidth - frame.clientWidth)
    const hasOverflow = contentOverflow > 0
    const previousMetrics = scrollMetricsRef.current
    const geometryChanged = maximumScroll !== previousMetrics.maximum
    const movedLeft = frame.scrollLeft < previousMetrics.left
    const movedRight = frame.scrollLeft > previousMetrics.left
    const contractedEdge = Math.max(0, previousMetrics.left - (previousMetrics.maximum - maximumScroll))
    const userMovedLeft = movedLeft && (maximumScroll >= previousMetrics.maximum || frame.scrollLeft < contractedEdge)
    if (explicitIntent === "left" || (rightEdgeIntentRef.current !== "none" && userMovedLeft)) {
      rightEdgeIntentRef.current = "none"
    } else if (explicitIntent === "right") {
      rightEdgeIntentRef.current = hasOverflow ? "anchored" : "pending"
      if (hasOverflow) frame.scrollLeft = maximumScroll
    } else if (!hasOverflow && rightEdgeIntentRef.current === "anchored") {
      rightEdgeIntentRef.current = "none"
    } else if (hasOverflow && rightEdgeIntentRef.current !== "none" && geometryChanged) {
      frame.scrollLeft = maximumScroll
      rightEdgeIntentRef.current = "anchored"
    }
    const atRightEdge = hasOverflow && frame.scrollLeft >= maximumScroll - edgeTolerance
    if (!geometryChanged && movedRight && atRightEdge) {
      rightEdgeIntentRef.current = "anchored"
    }
    const nextState = {
      hasOverflow,
      canScrollLeft: frame.scrollLeft > edgeTolerance,
      canScrollRight: frame.scrollLeft < maximumScroll - edgeTolerance,
    }
    scrollMetricsRef.current = { left: frame.scrollLeft, maximum: maximumScroll }
    setScrollState((current) => current.hasOverflow === nextState.hasOverflow
      && current.canScrollLeft === nextState.canScrollLeft
      && current.canScrollRight === nextState.canScrollRight ? current : nextState)
  }, [])

  useLayoutEffect(() => {
    if (!rendersTable) return
    const frame = frameRef.current
    const table = tableRef.current
    if (frame === null || table === null) return
    const updateScrolledState = () => updateScrollState()
    const updateLayoutState = () => updateScrollState()
    const observer = new ResizeObserver(updateLayoutState)
    const mutationObserver = new MutationObserver(updateLayoutState)
    observer.observe(frame)
    observer.observe(table)
    mutationObserver.observe(table, { childList: true, characterData: true, subtree: true })
    frame.addEventListener("scroll", updateScrolledState, { passive: true })
    document.fonts?.addEventListener("loadingdone", updateScrolledState)
    void document.fonts?.ready.then(updateScrolledState)
    updateScrollState()
    return () => {
      observer.disconnect()
      mutationObserver.disconnect()
      frame.removeEventListener("scroll", updateScrolledState)
      document.fonts?.removeEventListener("loadingdone", updateScrolledState)
    }
  }, [rendersTable, updateScrollState])

  return { frameRef, tableRef, scrollState, updateScrollState }
}
