import { useEffect, useState } from "react"

import type { ApiClient } from "../api/client"
import { type About, aboutSchema } from "../api/schemas"

/**
 * The running build, and the offer of its source.
 *
 * The version is read from the Controller, not from the bundle. In production
 * the bundle is embedded in the binary so the two agree, but in development a
 * proxied dev bundle may be talking to any build -- and the operator's question
 * is always about the server they are driving.
 *
 * The stamp is a link because this project ships under AGPL-3.0, whose section
 * 13 requires that whoever interacts with the program over a network be offered
 * its corresponding source. That obligation is why this line exists at all.
 *
 * Nothing renders until the read lands, and a failure stays silent: a build
 * stamp must never put an error in front of an operator working a task queue.
 */
export function BuildInfo({ apiClient }: { readonly apiClient: ApiClient }) {
  const [about, setAbout] = useState<About | null>(null)

  useEffect(() => {
    const controller = new AbortController()
    void apiClient.request("api/about", { schema: aboutSchema, signal: controller.signal }).then(
      (value) => {
        if (!controller.signal.aborted) setAbout(value)
      },
      () => {
        if (!controller.signal.aborted) setAbout(null)
      },
    )
    return () => controller.abort()
  }, [apiClient])

  if (about === null) return null
  return (
    <a
      className="build-info"
      href={about.source_url}
      target="_blank"
      rel="noopener noreferrer"
      aria-label={`${about.name} ${about.version}, source repository`}
    >
      v{about.version}
    </a>
  )
}
