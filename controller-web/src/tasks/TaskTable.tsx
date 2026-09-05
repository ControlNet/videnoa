import { ChevronLeft, ChevronRight } from "lucide-react"
import type { KeyboardEvent } from "react"

import type { TaskList } from "../api/taskSchemas"
import { formatBytes, formatDate, formatDuration, formatStatus } from "./format"
import { taskName } from "./model"
import { type OptionalColumn, optionalColumnLabels } from "./query"
import { type ScrollUpdate, useTaskTableScroll } from "./useTaskTableScroll"

type TaskTableProps = {
  readonly page: TaskList | null
  readonly columns: readonly OptionalColumn[]
  readonly loading: boolean
  readonly selectedTaskId?: string | null
  readonly onSelectTask?: (taskId: string) => void
}

const loadingRowKeys = ["one", "two", "three", "four", "five", "six", "seven", "eight"] as const
export function TaskTable({ page, columns, loading, selectedTaskId = null, onSelectTask }: TaskTableProps) {
  const rendersTable = page === null ? loading : page.items.length > 0
  const { frameRef, tableRef, scrollState, updateScrollState } = useTaskTableScroll(rendersTable)

  if (page === null && !loading) return null
  if (page !== null && page.items.length === 0) {
    return (
      <output className="task-empty" aria-live="polite">
        <strong>No tasks match this view.</strong>
        <span>Adjust the filters or wait for new work.</span>
      </output>
    )
  }

  return (
    <>
      {scrollState.hasOverflow ? (
        <nav className="task-table-scroll-controls" aria-label="Task table horizontal navigation">
          <p className="task-table-hint" id="task-table-scroll-hint">
            Use the arrow keys or table navigation controls to view hidden columns.
          </p>
          <div>
            <button type="button" title="Scroll task table left" aria-label="Scroll task table left" disabled={!scrollState.canScrollLeft} onClick={() => scrollTable(frameRef.current, "left", updateScrollState)}>
              <ChevronLeft size={16} aria-hidden="true" />
            </button>
            <button type="button" title="Scroll task table right" aria-label="Scroll task table right" disabled={!scrollState.canScrollRight} onClick={() => scrollTable(frameRef.current, "right", updateScrollState)}>
              <ChevronRight size={16} aria-hidden="true" />
            </button>
          </div>
        </nav>
      ) : null}
      <section
        ref={frameRef}
        className="task-table-frame"
        aria-label="Scrollable task results"
        aria-describedby={scrollState.hasOverflow ? "task-table-scroll-hint" : undefined}
        aria-busy={loading}
        tabIndex={scrollState.hasOverflow ? 0 : -1}
        onKeyDown={(event) => handleScrollKey(event, updateScrollState)}
      >
        <table ref={tableRef} className="task-table">
          <thead>
            <tr>
              <th scope="col">Status</th>
              <th scope="col">Name</th>
              <th scope="col">Workflow</th>
              <th scope="col">Worker</th>
              <th scope="col">Progress</th>
              <th scope="col">FPS</th>
              <th scope="col">ETA</th>
              <th scope="col">Size</th>
              <th scope="col">Created</th>
              <th scope="col">Finished</th>
              <th scope="col">Source</th>
              {columns.map((column) => (
                <th scope="col" key={column}>
                  {optionalColumnLabels[column]}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {page === null ? (
              <LoadingRows columns={columns.length} />
            ) : (
              page.items.map((task) => (
                <tr key={task.id} aria-selected={selectedTaskId === task.id ? true : undefined}>
                  <td>
                    <span className={`task-status ${task.status}`}>{formatStatus(task.status)}</span>
                  </td>
                  <td className="task-name" title={task.input_path}>
                    {onSelectTask === undefined ? (
                      taskName(task)
                    ) : (
                      <button
                        type="button"
                        className="task-row-select"
                        data-task-id={task.id}
                        aria-label={`Open task ${task.id}`}
                        onClick={() => onSelectTask(task.id)}
                      >
                        {taskName(task)}
                      </button>
                    )}
                  </td>
                  <td>{task.workflow}</td>
                  <td className="mono-cell" title={task.worker_id ?? undefined}>
                    {shortId(task.worker_id)}
                  </td>
                  <td>
                    <span className="progress-cell">
                      <span>
                        <i style={{ inlineSize: `${task.progress.percent}%` }} />
                      </span>
                      <b>{Math.round(task.progress.percent)}%</b>
                    </span>
                  </td>
                  <td className="numeric-cell">{task.progress.frames_per_second?.toFixed(1) ?? "--"}</td>
                  <td className="numeric-cell">{formatDuration(task.progress.eta_seconds)}</td>
                  <td className="numeric-cell">{formatBytes(task.input_size)}</td>
                  <td className="date-cell">{formatDate(task.created_at)}</td>
                  <td className="date-cell">{formatDate(task.completed_at)}</td>
                  <td>{task.source}</td>
                  {columns.map((column) => (
                    <OptionalCell key={column} column={column} task={task} />
                  ))}
                </tr>
              ))
            )}
          </tbody>
        </table>
      </section>
    </>
  )
}

function scrollTable(frame: HTMLElement | null, direction: "left" | "right", onScroll: ScrollUpdate): void {
  if (frame === null) return
  frame.scrollBy({ left: frame.clientWidth * (direction === "left" ? -0.75 : 0.75) })
  requestAnimationFrame(() => onScroll())
}

function handleScrollKey(event: KeyboardEvent<HTMLElement>, onScroll: ScrollUpdate): void {
  if (event.target !== event.currentTarget) return
  const frame = event.currentTarget
  switch (event.key) {
    case "ArrowLeft":
      event.preventDefault()
      scrollTable(frame, "left", onScroll)
      break
    case "ArrowRight":
      event.preventDefault()
      scrollTable(frame, "right", onScroll)
      break
    case "Home":
      event.preventDefault()
      frame.scrollLeft = 0
      onScroll("left")
      break
    case "End":
      event.preventDefault()
      onScroll("right")
      break
  }
}

function OptionalCell({ column, task }: { readonly column: OptionalColumn; readonly task: TaskList["items"][number] }) {
  switch (column) {
    case "input_path":
      return (
        <td className="long-cell mono-cell" title={task.input_path}>
          {task.input_path}
        </td>
      )
    case "output_path":
      return (
        <td className="long-cell mono-cell" title={task.output_path}>
          {task.output_path}
        </td>
      )
    case "attempts":
      return <td className="numeric-cell">{task.attempt_count}</td>
    case "duration":
      return (
        <td className="numeric-cell">
          {formatDuration((new Date(task.completed_at ?? task.updated_at).getTime() - new Date(task.created_at).getTime()) / 1000)}
        </td>
      )
    case "failure_stage":
      return <td>{task.failure?.failure_stage ?? "--"}</td>
    case "failure":
      return <td>{task.failure?.failure_code ?? "--"}</td>
    case "error":
      return (
        <td className="long-cell" title={task.failure?.message}>
          {task.failure?.message ?? "--"}
        </td>
      )
    case "remote_job_id":
      return (
        <td className="mono-cell" title={task.remote_job_id ?? undefined}>
          {shortId(task.remote_job_id)}
        </td>
      )
  }
}

function LoadingRows({ columns }: { readonly columns: number }) {
  return loadingRowKeys.map((key) => (
    <tr className="loading-row" key={key}>
      <td colSpan={11 + columns}>
        <span />
      </td>
    </tr>
  ))
}

function shortId(value: string | null): string {
  return value === null ? "--" : value.slice(0, 8)
}
