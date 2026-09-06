/**
 * Semantic status tones.
 *
 * The product has fourteen task statuses, three worker health/policy pairs and
 * several readiness states. Rendering each as its own coloured block turns a
 * dense table into noise, so everything resolves to one of four tones and the
 * exact state stays in the text beside the dot.
 */
export type Tone = "quiet" | "active" | "positive" | "negative"

type StatusProps = {
  readonly tone: Tone
  readonly label: string
  /** Adds the live halo. Reserved for a verified open connection. */
  readonly live?: boolean
  readonly title?: string
}

export function Status({ tone, label, live = false, title }: StatusProps) {
  return (
    <span className={`status status--${tone}`} title={title}>
      <span className={live ? "dot dot--live" : "dot"} aria-hidden="true" />
      <span>{label}</span>
    </span>
  )
}

export function Dot({ tone, live = false }: { readonly tone: Tone; readonly live?: boolean }) {
  return <span className={`status status--${tone}`} aria-hidden="true"><span className={live ? "dot dot--live" : "dot"} /></span>
}
