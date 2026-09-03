import type { TaskCreateRequest } from "../api/taskSchemas"

export type SubmissionIntent = {
  readonly body: TaskCreateRequest
  readonly fingerprint: string
  readonly key: string
  readonly state: "ready" | "ambiguous"
}

export function beginSubmission(current: SubmissionIntent | null, body: TaskCreateRequest, randomUUID: () => string): SubmissionIntent {
  const fingerprint = JSON.stringify(body)
  if (current?.state === "ambiguous" && current.fingerprint === fingerprint) return current
  return { body, fingerprint, key: randomUUID(), state: "ready" }
}

export function markSubmissionAmbiguous(intent: SubmissionIntent): SubmissionIntent {
  return { ...intent, state: "ambiguous" }
}
