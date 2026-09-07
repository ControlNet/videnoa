# Local font packaging

## Result

- Worker and Controller now load `/fonts/fonts.css` from their own origin. Google Fonts stylesheet and preconnect tags were removed from both entry HTML files. Controller design previews use `../public/fonts/fonts.css`.
- Each frontend's `public/fonts/` contains 12 unmodified variable WOFF2 subsets: Geist Mono v6 (weights 400..600) and Manrope v20 (weights 300..800), covering the union of existing frontend requests. Upstream unicode ranges and `font-display: swap` are retained; Chinese text continues to use system fallbacks.
- Both families' upstream SIL Open Font License notices are included. `SOURCES.md` records URLs and SHA-256 hashes. Its external URLs are provenance text, not browser resource requests.
- Per frontend: 145,420 bytes of WOFF2 data; 161,030 bytes (157.3 KiB) including CSS, both license notices, and provenance. Both independently bundled copies total 322,060 bytes (314.5 KiB), before archive/container compression.
- Vite copies all 16 files into `dist/fonts/`. Existing release embedding and Docker COPY paths include the entire dist directory, so no separate model/resource download step is required for fonts.
- Both Rust release build scripts now track the frontend `public/` directory, so changing the vendored assets invalidates the frontend build.
- No npm dependency, remote font fetch at build time, or system Python package was introduced. The one-time font download used Python's standard library.

This supersedes the Google Fonts runtime-dependency findings in `frontend-google-font-dependency-2026-09-07.md` and `external-network-dependencies-2026-09-07.md`. The earlier size estimate is replaced by the measured vendored totals above.

## Verification

Both lint/build pairs and formatting checks passed:

```bash
npm --prefix web run lint
npm --prefix web run build
npm --prefix controller-web run lint
npm --prefix controller-web run build
rustfmt --check --edition 2021 crates/core/build.rs crates/controller/build.rs
git diff --check
```

Compared all 16 source files byte-for-byte with each frontend's built `dist/fonts/` directory. Both built entry pages reference the local stylesheet; the stylesheet contains 12 relative font URLs and no external URLs.

A temporary Playwright/Chromium check served each real built frontend with Vite preview, blocked all off-origin HTTP(S) requests, loaded the page, and called `load()` on every face in `document.fonts`. Both frontends decoded all 12 faces, received 12 successful same-origin WOFF2 responses, and attempted zero external requests. No API mocks or synthetic font files were used. This checked font loading, not backend functionality.

Existing non-fatal build warnings remain: Worker JS chunk size and Controller dependency PURE-comment annotations. No release archive/image or NAS deployment was produced during this change; subsequent normal builds include the local assets.
