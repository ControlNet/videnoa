import type { TaskCreateRequest } from "../api/taskSchemas"

export type SubmissionIntent = {
  readonly body: TaskCreateRequest
  readonly fingerprint: string
  readonly key: string
  readonly state: "ready" | "ambiguous"
}

function submissionKey(): string {
  // getRandomValues also works on LAN HTTP origins, where randomUUID is unavailable.
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  const hex = Array.from(bytes, (byte, index) => {
    if (index === 6) byte = (byte & 0x0f) | 0x40
    if (index === 8) byte = (byte & 0x3f) | 0x80
    return byte.toString(16).padStart(2, "0")
  }).join("")
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}

export function beginSubmission(current: SubmissionIntent | null, body: TaskCreateRequest, randomUUID: () => string = submissionKey): SubmissionIntent {
  const fingerprint = JSON.stringify(body)
  if (current?.state === "ambiguous" && current.fingerprint === fingerprint) return current
  return { body, fingerprint, key: randomUUID(), state: "ready" }
}

export function markSubmissionAmbiguous(intent: SubmissionIntent): SubmissionIntent {
  return { ...intent, state: "ambiguous" }
}
