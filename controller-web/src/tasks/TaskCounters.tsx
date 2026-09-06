import type { TaskStatusCounts } from "../api/taskSchemas"
import { counterValues } from "./model"

type TaskCountersProps = {
  readonly counts: TaskStatusCounts | null
}

/**
 * Status counts as one inline strip.
 *
 * Counts are context for the table beneath them, not a dashboard: the number
 * carries the weight and the label stays quiet, so the whole set costs one row.
 */
export function TaskCounters({ counts }: TaskCountersProps) {
  const values = counts === null ? null : counterValues(counts)
  const counters = [
    ["All", values?.all, "total"],
    ["Queued", values?.queued, "quiet"],
    ["Active", values?.active, "active"],
    ["Processing", values?.processing, "active"],
    ["Failed", values?.failed, "negative"],
    ["Finished", values?.finished, "quiet"],
  ] as const

  return (
    <dl className="task-counters" aria-label="Task status counts" aria-live="polite" aria-atomic="true">
      {counters.map(([label, value, tone]) => (
        <div key={label} className={`counter counter--${tone}`}>
          {/* Source order stays label-then-value for announcement; CSS shows the value first. */}
          <dt>{label}</dt>
          <dd>{value?.toLocaleString() ?? "--"}</dd>
        </div>
      ))}
    </dl>
  )
}
