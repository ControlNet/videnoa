export function App() {
  return (
    <main className="controller-shell">
      <article className="controller-panel" aria-labelledby="controller-title">
        <header className="product-header">
          <span className="product-mark" aria-hidden="true">
            V
          </span>
          <p className="product-name">Videnoa Controller</p>
          <p className="product-role">Coordination service</p>
        </header>

        <section className="workspace-intro">
          <p className="section-label">Independent service boundary</p>
          <h1 id="controller-title">Controller workspace</h1>
          <p className="workspace-summary">
            A GPU-free control plane for coordinating Videnoa processing services across your NAS.
          </p>
        </section>

        <footer className="service-status">
          <output className="status-message" aria-label="Controller service status">
            <span className="status-indicator" aria-hidden="true" />
            <span className="status-label">Service online</span>
          </output>
          <code>/api/health</code>
        </footer>
      </article>
    </main>
  )
}
