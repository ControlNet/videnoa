import type { TaskStatusCounts } from "../api/taskSchemas"
import { type Counter, Counters } from "../ui/Counters"
import { counterValues } from "./model"

type TaskCountersProps = {
  readonly counts: TaskStatusCounts | null
}

export function TaskCounters({ counts }: TaskCountersProps) {
  const values = counts === null ? null : counterValues(counts)
  const counters: readonly Counter[] = [
    { label: "All", value: format(values?.all), tone: "total" },
    { label: "Queued", value: format(values?.queued), tone: "quiet" },
    { label: "Active", value: format(values?.active), tone: "active" },
    { label: "Processing", value: format(values?.processing), tone: "active" },
    { label: "Failed", value: format(values?.failed), tone: "negative" },
    { label: "Finished", value: format(values?.finished), tone: "quiet" },
  ]
  return <Counters label="Task status counts" counters={counters} />
}

function format(value: number | undefined): string | null {
  return value === undefined ? null : value.toLocaleString()
}
