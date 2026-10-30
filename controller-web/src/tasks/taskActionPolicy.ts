import type { FailureCode, FailureStage, Task } from "../api/taskSchemas"

export type FailureGuidance = {
  readonly kind: "blocked" | "new_task" | "processing_retry" | "stage_retry"
  readonly message: string
}

/*
 * The Controller lists the actions each task accepts in `allowed_actions`, so
 * the UI offers exactly those and never re-derives lifecycle rules. Actions this
 * UI does not know are ignored.
 */
function allows(task: Task, action: string): boolean {
  return task.allowed_actions.includes(action)
}

export function canCancelTask(task: Task): boolean {
  return allows(task, "cancel")
}

/** A requested cancellation that waits for a Worker that cannot answer. */
export function canAbandonTask(task: Task): boolean {
  return allows(task, "abandon")
}

export function canRetryTask(task: Task): boolean {
  return allows(task, "retry")
}

/**
 * Whether the retry may move to another Worker: only failures before any output
 * exists. The Controller deletes the task workspace on the original Worker first.
 */
export function canRetryOnAnotherWorker(task: Task): boolean {
  return allows(task, "retry_on_another_worker")
}

/** Give up on a failed task: it is never retried and its files are released. */
export function canDiscardTask(task: Task): boolean {
  return allows(task, "discard")
}

export function failureGuidance(code: FailureCode, stage: FailureStage): FailureGuidance {
  switch (code) {
    case "input_changed":
      return stage === "upload"
        ? { kind: "stage_retry", message: "Retry uploads the current file at the existing input path. Changes since task creation are accepted." }
        : { kind: "blocked", message: "This input failure cannot be retried from its recorded stage." }
    case "output_exists":
    case "input_unavailable":
      return {
        kind: "new_task",
        message: "Changing an input or output path, including resolving an output collision, requires creating a new task. Retry never changes paths.",
      }
    case "processing_failed":
      return {
        kind: "processing_retry",
        message: "Processing retry first verifies the remote job is terminal and the task workspace is clean, then queues the task for a new processing attempt on the same Worker, or on the Worker chosen from the Retry menu.",
      }
    case "transfer_failed":
    case "verification_failed":
    case "publication_failed":
    case "cleanup_failed":
      return {
        kind: "stage_retry",
        message: `Retry resumes the failed ${stage.replaceAll("_", " ")} stage without repeating completed AI processing or changing paths.`,
      }
    case "remote_state_ambiguous":
      return {
        kind: "blocked",
        message: "Remote state is ambiguous. Verify the remote job and workspace manually; automatic retry is blocked to prevent duplicate processing.",
      }
    case "publication_ambiguous":
      return {
        kind: "stage_retry",
        message: "Inspect the destination and verified temporary file before retrying publication. Manual retry rechecks ownership and content without overwriting conflicting files or repeating AI processing.",
      }
    case "remote_submission_failed":
      return stage === "submission"
        ? { kind: "stage_retry", message: "The worker rejected the submission. Fix the reported cause on the worker, then retry: the uploaded input is submitted again on the same attempt without re-uploading or changing paths." }
        : { kind: "blocked", message: "This submission failure cannot be retried from its recorded stage." }
    case "worker_unavailable":
    case "workflow_incompatible":
    case "cancelled":
      return { kind: "blocked", message: "This failure is not retryable. Resolve the reported condition and create a new task if task inputs must change." }
  }
}
