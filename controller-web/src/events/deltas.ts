/**
 * The recent deltas of one kind, kept so a subscriber that renders once for
 * several publishes still sees each of them.
 *
 * React may batch two publishes into one render. A store that kept only the
 * latest delta then lost the earlier one, e.g. a second task's progress
 * replaced the first's, and that row stayed stale until the next refetch.
 */
export type Delta<T> = {
  readonly generation: number
  readonly value: T
}

/** Deltas kept per store; a subscriber further behind than this refetches. */
export const RECENT_DELTA_LIMIT = 256

export function appendDelta<T>(recent: readonly Delta<T>[], delta: Delta<T>): readonly Delta<T>[] {
  const next = [...recent, delta]
  return next.length > RECENT_DELTA_LIMIT ? next.slice(next.length - RECENT_DELTA_LIMIT) : next
}

/**
 * The values published after `applied`, oldest first. `complete` is false when
 * some of them have already been dropped, so the caller must refetch.
 */
export function deltasSince<T>(
  recent: readonly Delta<T>[],
  applied: number,
): { readonly values: readonly T[]; readonly complete: boolean } {
  const fresh = recent.filter((delta) => delta.generation > applied)
  const complete = fresh.length === 0 || fresh[0]?.generation === applied + 1
  return { values: fresh.map((delta) => delta.value), complete }
}
