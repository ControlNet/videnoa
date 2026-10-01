# Videnoa Worker WebUI

React + TypeScript single-page app for the Videnoa Worker: the node-based
workflow editor, presets, job queue and history, model management, performance
view and settings. It talks to the Worker's HTTP/WebSocket API (`/api/...`) and
is embedded into the `videnoa` binary at build time, so a release build of the
Rust workspace serves it from `web/dist`.

Stack: Vite 7, React 19, Zustand, React Flow (`@xyflow/react`), Tailwind CSS 4,
i18next, Vitest.

## Commands

Requires Node.js 24 (the version CI and the Docker build use).

```bash
cd web
npm ci              # install dependencies from package-lock.json
npm run dev         # Vite dev server with HMR (proxy the Worker API as needed)
npm run lint        # ESLint
npm test            # Vitest unit tests (vitest run)
npm run build       # type-check (tsc -b) and build to web/dist
npm run preview     # serve the production build locally
```

During development, start the Worker from the repository root
(`./target/release/videnoa` or `cargo run -p videnoa-app`) and run `npm run dev`
alongside it; the production build is served by the Worker itself.

## Layout

- `src/api/` - typed client for the Worker API
- `src/stores/` - Zustand stores (workflow, jobs, models, settings)
- `src/components/` - UI components, including the node editor
- `src/lib/` - shared helpers (kept tracked despite the global `lib/` ignore)
- `design/` - design references; `videnoa-worker-gui.html` is a generated
  canvas dump and is not tracked
