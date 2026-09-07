export type CounterTone = "total" | "quiet" | "active" | "positive" | "negative"

export type Counter = {
  readonly label: string
  /** Already formatted; `null` while the source is still loading. */
  readonly value: string | null
  readonly tone: CounterTone
}

type CountersProps = {
  readonly label: string
  readonly counters: readonly Counter[]
}

/**
 * A route's headline numbers as one inline strip.
 *
 * These are context for the surface beneath them, not a dashboard: the value
 * carries the weight, the label stays quiet, and the whole set costs one row.
 */
export function Counters({ label, counters }: CountersProps) {
  return (
    <dl className="counters" aria-label={label} aria-live="polite" aria-atomic="true">
      {counters.map((counter) => (
        <div key={counter.label} className={`counter counter--${counter.tone}`}>
          {/* Source order stays label-then-value for announcement; CSS shows the value first. */}
          <dt>{counter.label}</dt>
          <dd>{counter.value ?? "--"}</dd>
        </div>
      ))}
    </dl>
  )
}
