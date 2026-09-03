import type { TaskAttempt } from "../api/taskSchemas"
import { formatDate, formatDuration, formatStatus } from "./format"

type TaskAttemptsProps = {
  readonly attempts: readonly TaskAttempt[]
}

export function TaskAttempts({ attempts }: TaskAttemptsProps) {
  if (attempts.length === 0) return <p className="detail-empty">No attempts have been persisted.</p>
  return (
    <ol className="task-attempts">
      {attempts.map((attempt) => (
        <li key={attempt.id}>
          <header>
            <strong>Attempt {attempt.attempt_number}</strong>
            <span className={`task-status ${attempt.status}`}>{formatStatus(attempt.status)}</span>
          </header>
          <dl className="detail-grid">
            <Detail label="Attempt ID" value={attempt.id} mono />
            <Detail label="Worker" value={attempt.worker_id} mono />
            <Detail label="Submission Key" value={attempt.submission_key} mono />
            <Detail label="Remote Job" value={attempt.remote_job_id} mono />
            <Detail label="Remote Input" value={attempt.remote_input_path} mono />
            <Detail label="Remote Output" value={attempt.remote_output_path} mono />
            <Detail label="Retry Count" value={attempt.retry.retry_count.toLocaleString()} />
            <Detail label="Next Retry" value={formatDate(attempt.retry.next_retry_at)} />
            <Detail label="Progress" value={`${attempt.progress.percent.toLocaleString()}%`} />
            <Detail label="FPS" value={attempt.progress.frames_per_second?.toLocaleString() ?? "--"} />
            <Detail label="ETA" value={formatDuration(attempt.progress.eta_seconds)} />
            <Detail label="Created" value={formatDate(attempt.created_at)} />
            <Detail label="Started" value={formatDate(attempt.started_at)} />
            <Detail label="Completed" value={formatDate(attempt.completed_at)} />
            <Detail label="Duration" value={formatDuration(durationSeconds(attempt))} />
          </dl>
          {attempt.failure === null ? null : (
            <p className="attempt-failure">
              <strong>{attempt.failure.failure_code}</strong> {attempt.failure.message}
            </p>
          )}
        </li>
      ))}
    </ol>
  )
}

export function Detail({ label, value, mono = false }: { readonly label: string; readonly value: string | null; readonly mono?: boolean }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd className={mono ? "copy-value" : undefined} title={value ?? undefined}>
        {value ?? "--"}
      </dd>
    </div>
  )
}

function durationSeconds(attempt: TaskAttempt): number | null {
  if (attempt.started_at === null || attempt.completed_at === null) return null
  return (new Date(attempt.completed_at).getTime() - new Date(attempt.started_at).getTime()) / 1000
}
