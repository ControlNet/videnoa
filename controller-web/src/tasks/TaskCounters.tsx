import type { TaskStatusCounts } from "../api/taskSchemas"
import { counterValues } from "./model"

type TaskCountersProps = {
  readonly counts: TaskStatusCounts | null
}

export function TaskCounters({ counts }: TaskCountersProps) {
  const values = counts === null ? null : counterValues(counts)
  const counters = [
    ["All", values?.all],
    ["Active", values?.active],
    ["Queued", values?.queued],
    ["Processing", values?.processing],
    ["Failed", values?.failed],
    ["Finished", values?.finished],
  ] as const

  return (
    <dl className="task-counters" aria-label="Task status counts" aria-live="polite" aria-atomic="true">
      {counters.map(([label, value]) => (
        <div key={label}>
          <dt>{label}</dt>
          <dd>{value?.toLocaleString() ?? "--"}</dd>
        </div>
      ))}
    </dl>
  )
}
