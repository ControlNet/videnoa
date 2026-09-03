import type { TaskDetail } from "../api/taskSchemas"
import { formatBytes, formatDate, formatDuration } from "./format"
import { Detail, TaskAttempts } from "./TaskAttempts"
import type { FailureGuidance } from "./taskActionPolicy"

type TaskDetailContentProps = {
  readonly detail: TaskDetail
  readonly guidance: FailureGuidance | null
}

export function TaskDetailContent({ detail, guidance }: TaskDetailContentProps) {
  return (
    <div className="task-detail-content">
      <section>
        <h3>General</h3>
        <dl className="detail-grid">
          <Detail label="Task ID" value={detail.task.id} mono />
          <Detail label="Version" value={String(detail.task.version)} />
          <Detail label="Input Path" value={detail.task.input_path} mono />
          <Detail label="Output Path" value={detail.task.output_path} mono />
          <Detail label="Workflow" value={detail.task.workflow} />
          <Detail label="Priority" value={detail.task.priority.toLocaleString()} />
          <Detail label="Source" value={detail.task.source} />
          <Detail label="Source Reference" value={detail.task.source_reference} mono />
          <Detail label="Input Size" value={formatBytes(detail.task.input_size)} />
          <Detail label="Cancel Requested" value={formatDate(detail.task.cancel_requested_at)} />
          <Detail label="Worker" value={detail.task.worker_id} mono />
          <Detail label="Remote Job" value={detail.task.remote_job_id} mono />
          <Detail label="Created" value={formatDate(detail.task.created_at)} />
          <Detail label="Updated" value={formatDate(detail.task.updated_at)} />
          <Detail label="Completed" value={formatDate(detail.task.completed_at)} />
          <Detail label="Duration" value={formatDuration(taskDuration(detail.task.created_at, detail.task.completed_at ?? detail.task.updated_at))} />
        </dl>
      </section>
      <section>
        <h3>Progress</h3>
        <dl className="detail-grid">
          <Detail label="Percent" value={`${detail.task.progress.percent.toLocaleString()}%`} />
          <Detail label="Frames" value={frameProgress(detail.task.progress.processed_frames, detail.task.progress.total_frames)} />
          <Detail label="FPS" value={detail.task.progress.frames_per_second?.toLocaleString() ?? "--"} />
          <Detail label="ETA" value={formatDuration(detail.task.progress.eta_seconds)} />
          <Detail label="Transferred" value={optionalBytes(detail.task.progress.bytes_transferred)} />
          <Detail label="Total Bytes" value={optionalBytes(detail.task.progress.bytes_total)} />
        </dl>
      </section>
      <section className="attempt-section">
        <h3>Attempts</h3>
        <TaskAttempts attempts={detail.attempts} />
      </section>
      <section>
        <h3>Error / Logs</h3>
        {detail.task.failure === null ? (
          <p className="detail-empty">No persisted failure. The Controller API does not expose server logs for this task.</p>
        ) : (
          <div className="failure-detail">
            <dl className="detail-grid">
              <Detail label="Stage" value={detail.task.failure.failure_stage} />
              <Detail label="Code" value={detail.task.failure.failure_code} mono />
              <Detail label="Retryable" value={detail.task.failure.retryable ? "Yes" : "No"} />
            </dl>
            <p className="copy-value">{detail.task.failure.message}</p>
            {guidance === null ? null : <p className="failure-guidance">{guidance.message}</p>}
          </div>
        )}
      </section>
    </div>
  )
}

function taskDuration(start: string, end: string): number {
  return (new Date(end).getTime() - new Date(start).getTime()) / 1000
}

function frameProgress(processed: number | null, total: number | null): string {
  if (processed === null && total === null) return "--"
  return `${processed?.toLocaleString() ?? "--"} / ${total?.toLocaleString() ?? "--"}`
}

function optionalBytes(value: number | null): string {
  return value === null ? "--" : formatBytes(value)
}
