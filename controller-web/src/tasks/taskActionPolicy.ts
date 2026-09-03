import type { FailureCode, FailureStage, Task } from "../api/taskSchemas"

const cancellableStatuses = new Set<Task["status"]>([
  "queued",
  "reserved",
  "uploading",
  "staged",
  "submitting",
  "processing",
  "remote_completed",
  "downloading",
  "verifying",
])

export type FailureGuidance = {
  readonly kind: "blocked" | "new_task" | "processing_retry" | "stage_retry"
  readonly message: string
}

export function canCancelTask(task: Task): boolean {
  return task.cancel_requested_at === null && cancellableStatuses.has(task.status)
}

export function canRetryTask(task: Task): boolean {
  if (task.status !== "failed" || task.failure?.retryable !== true) return false
  return isSupportedRetryPair(task.failure.failure_code, task.failure.failure_stage)
}

function isSupportedRetryPair(code: FailureCode, stage: FailureStage): boolean {
  switch (code) {
    case "processing_failed":
      return stage === "processing"
    case "transfer_failed":
      return stage === "upload" || stage === "download"
    case "verification_failed":
      return stage === "verification"
    case "publication_failed":
      return stage === "publication"
    case "cleanup_failed":
      return stage === "local_cleanup" || stage === "remote_cleanup"
    case "input_unavailable":
    case "input_changed":
    case "output_exists":
    case "worker_unavailable":
    case "workflow_incompatible":
    case "remote_submission_failed":
    case "remote_state_ambiguous":
    case "publication_ambiguous":
    case "cancelled":
      return false
  }
}

export function failureGuidance(code: FailureCode, stage: FailureStage): FailureGuidance {
  switch (code) {
    case "output_exists":
    case "input_unavailable":
    case "input_changed":
      return {
        kind: "new_task",
        message: "Changing an input or output path, including resolving an output collision, requires creating a new task. Retry never changes paths.",
      }
    case "processing_failed":
      return {
        kind: "processing_retry",
        message: "Processing retry first verifies the remote job is terminal and the task workspace is clean, then starts a new processing attempt.",
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
        kind: "blocked",
        message: "Publication is ambiguous. Inspect the destination and staging artifact before taking further action; retry is blocked.",
      }
    case "worker_unavailable":
    case "workflow_incompatible":
    case "remote_submission_failed":
    case "cancelled":
      return { kind: "blocked", message: "This failure is not retryable. Resolve the reported condition and create a new task if task inputs must change." }
  }
}
