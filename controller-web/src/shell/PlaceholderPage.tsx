type PlaceholderPageProps = {
  readonly description: string
  readonly nextTask: string
  readonly title: string
}

export function PlaceholderPage({ description, nextTask, title }: PlaceholderPageProps) {
  return (
    <div className="route-page">
      <header className="route-header">
        <p className="technical-label">CONTROLLER WORKSPACE</p>
        <h1>{title}</h1>
        <p>{description}</p>
      </header>
      <section className="readiness-panel" aria-labelledby={`${title.toLowerCase()}-readiness`}>
        <div>
          <span className="readiness-index">{nextTask}</span>
          <h2 id={`${title.toLowerCase()}-readiness`}>Interface boundary ready</h2>
        </div>
        <p>
          Authentication, protected routing, session recovery, and live invalidation are active. Domain controls arrive in the dedicated implementation task.
        </p>
      </section>
    </div>
  )
}
