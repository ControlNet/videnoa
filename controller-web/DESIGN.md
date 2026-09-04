# Videnoa Controller Design System

## 1. Atmosphere & Identity

Videnoa Controller is a compact industrial control surface for operators coordinating durable video-processing work. It uses Videnoa's Manrope and Geist Mono typography, cool graphite layers, a restrained violet focus accent, and one green live-state signal. Linear's luminance stacking informs the shell, but the result remains explicitly Videnoa: practical, quiet, and built around the Rust service boundary.

The memorable moment is the transition from the isolated sign-in panel into a fixed operational frame. Navigation remains stable while the route body becomes the sole scroll owner.

## 2. Color

| Role | Token | Light | Dark | Usage |
|---|---|---|---|---|
| Canvas | `--color-background` | `oklch(0.97 0.008 260)` | `oklch(0.12 0.022 260)` | Page and shell canvas |
| Canvas depth | `--color-background-deep` | `oklch(0.94 0.012 260)` | `oklch(0.095 0.025 260)` | Atmospheric edge |
| Surface | `--color-surface` | `oklch(0.995 0.003 260)` | `oklch(0.155 0.024 260)` | Sidebar and login panel |
| Elevated | `--color-surface-elevated` | `oklch(1 0 0)` | `oklch(0.19 0.025 260)` | Inputs and route callouts |
| Hover | `--color-surface-hover` | `oklch(0.93 0.014 260)` | `oklch(0.23 0.025 260)` | Interactive hover |
| Text | `--color-text` | `oklch(0.18 0.02 260)` | `oklch(0.96 0.008 260)` | Primary copy |
| Muted text | `--color-text-muted` | `oklch(0.45 0.018 260)` | `oklch(0.72 0.016 260)` | Supporting copy |
| Quiet text | `--color-text-quiet` | `oklch(0.48 0.014 260)` | `oklch(0.61 0.016 260)` | Metadata |
| Border | `--color-border` | `oklch(0.86 0.012 260)` | `oklch(0.29 0.02 260)` | Structural separation |
| Border subtle | `--color-border-subtle` | `oklch(0.91 0.008 260)` | `oklch(0.23 0.018 260)` | Recessed divisions |
| Accent | `--color-accent` | `oklch(0.51 0.2 292)` | `oklch(0.66 0.19 292)` | Focus, active route, primary action |
| Accent strong | `--color-accent-strong` | `oklch(0.46 0.21 292)` | `oklch(0.72 0.17 292)` | Accent hover |
| Accent wash | `--color-accent-wash` | `oklch(0.92 0.035 292)` | `oklch(0.22 0.055 292)` | Active navigation surface |
| Healthy | `--color-healthy` | `oklch(0.44 0.15 145)` | `oklch(0.72 0.16 145)` | Connected service state |
| Danger | `--color-danger` | `oklch(0.48 0.18 25)` | `oklch(0.7 0.17 25)` | Recoverable error text |
| Danger wash | `--color-danger-wash` | `oklch(0.94 0.025 25)` | `oklch(0.2 0.04 25)` | Error summary |

Rules:
- All color values are declared as semantic custom properties in `src/index.css`.
- Violet is reserved for identity, focus, active navigation, and primary actions.
- Green is reserved for verified connected state. Error red is always paired with explicit text.
- The application follows `prefers-color-scheme`; Task 15 adds no stored theme preference.

## 3. Typography

| Level | Token | Size | Weight | Line height | Usage |
|---|---|---|---|---|---|
| Route title | `--type-title` | `clamp(1.75rem, 4vw, 2.5rem)` | 600 | 1.1 | Route heading |
| Login title | `--type-login-title` | `clamp(2rem, 6vw, 3rem)` | 600 | 1.05 | Sign-in heading |
| Subtitle | `--type-subtitle` | `1.125rem` | 600 | 1.4 | Readiness heading |
| Body | `--type-body` | `1rem` | 400 | 1.6 | Descriptions |
| UI | `--type-ui` | `0.875rem` | 600 | 1.4 | Navigation and controls |
| Label | `--type-label` | `0.75rem` | 650 | 1.4 | Field labels and metadata |
| Micro | `--type-micro` | `0.6875rem` | 600 | 1.4 | Service boundary labels |

Font stacks:
- Body and display: `Manrope`, `Avenir Next`, `Segoe UI`, sans-serif.
- Product and technical labels: `Geist Mono`, `SFMono-Regular`, `Consolas`, monospace.
- Titles use tight negative tracking; body and UI copy use normal tracking.

## 4. Spacing & Layout

The base unit is 4px. Tokens are `--space-1` through `--space-12` at 4, 8, 12, 16, 20, 24, 32, and 48px.

The authenticated root is a `fixed-sidenav-shell` bounded by `100dvb`. The sidebar and mobile header stay fixed in their grid region; `.shell-main` is the only vertical scroll owner and therefore uses `min-block-size: 0; overflow: auto`. Content is limited to `72rem` and reflows without horizontal scrolling at 375px.

At widths below 48rem, the frame becomes two rows. Navigation is a horizontally scrollable, keyboard-accessible cluster, labels remain visible, and the route body keeps full-width gutters.

## 5. Components

### Login Panel
- **Structure**: product mark, purpose statement, labelled password field, inline error summary, primary submit.
- **States**: idle, submitting, wrong password, malformed response, network failure, rate limited.
- **Accessibility**: password receives initial focus; errors use `role="alert"` and receive programmatic focus; submit state is announced through its label.
- **Security**: password exists only in component and request memory and is cleared after success.

### Application Frame
- **Structure**: product identity, primary navigation, service state, sign-out action, scrolling route main.
- **States**: authenticated, recoverable sign-out failure, and session-expired. A failed sign-out keeps the authenticated frame mounted and focuses a retryable alert; expiry replaces the entire frame with login.
- **Accessibility**: labelled primary navigation; current route uses `aria-current="page"`; route changes focus the main landmark.
- **Layout**: fixed sidebar at desktop, fixed top region plus horizontal navigation on narrow screens.

### Application Error Boundary
- **Structure**: isolated recovery panel with an explicit interruption message and primary retry action.
- **States**: inactive during normal rendering and active after an unexpected descendant render failure.
- **Accessibility**: the retry action receives focus and supports keyboard recovery without requiring a page reload.

### Navigation Item
- **Structure**: one Lucide icon and a persistent text label.
- **States**: default, hover, active, keyboard focus.
- **Motion**: color and background transition using `--motion-fast`; no layout movement.

### Primary Button
- **Structure**: label with optional Lucide icon.
- **States**: default, hover, active, focus, disabled/submitting.
- **Motion**: 1px active translation for tactile feedback; removed under reduced motion.

### Field
- **Structure**: visible label, input, optional supporting or error text.
- **States**: idle, hover, focus, disabled, invalid.
- **Accessibility**: no placeholder-as-label; focus ring exceeds the component edge.

### Route Placeholder
- **Structure**: route heading, concise ownership description, one bordered readiness panel.
- **Scope**: intentionally excludes Tasks table, task creation/detail, worker operations, and settings controls assigned to Tasks 16-18.

### Task History Surface
- **Structure**: compact status counters, one bounded filter toolbar, a semantic dense table, and server-page navigation.
- **States**: loading rows, populated page, empty filter result, recoverable load failure, and live active-row replacement.
- **Density**: numeric and identifier cells use Geist Mono; row separators replace cards; long values truncate with native title disclosure.
- **Responsiveness**: the route never owns horizontal overflow; the table frame is the deliberate inline scroll region whenever measured content width exceeds its client width. A concise associated hint and compact boundary-aware navigation remain visible above the frame while overflow exists at any viewport width, and the named frame becomes a visible-focus keyboard scroll stop.
- **Live data**: matching active task deltas replace only newer row versions; membership or ordering changes refetch the bounded current page and counts.

### Manual Task Intake
- **Structure**: a compact native modal with exact input/output paths, workflow, integer priority, and explicit manual source semantics.
- **Idempotency**: the client keeps one in-memory UUID only for an unchanged request whose response was lost. Any field edit or confirmed API response ends that intent; a changed submission receives a new key.
- **Errors**: boundary validation uses the closed server field-error code set, preserves adjacent messages, and moves focus to the first invalid control. Structured path messages drive outside-root and no-clobber guidance even when the top-level message is generic. Ambiguous transport failure offers an explicit `Retry Same Task`; key/body conflicts explain that the next submission is a new intent.
- **Accessibility**: native modal focus containment, Escape dismissal, visible labels, and trigger-focus restoration support keyboard-only operation.

### Task Detail Inspector
- **Structure**: selecting a task opens a table-adjacent bottom pane containing authoritative general data, progress, persisted attempts, and failure evidence.
- **Authority**: list rows and SSE deltas select or invalidate; bounded `GET /api/tasks/{id}?limit=&offset=` pages remain the source of truth for versions, attempts, and action eligibility. The inspector loads 100 newest attempts first and exposes an explicit next-page action while more persisted history exists.
- **History concurrency**: each next-page request is owned by the selected task, detail generation, and requested offset. Selection changes, manual reloads, and SSE invalidation abort pending history work; ownership checks reject late transport completions, while ID deduplication preserves newest-to-oldest order and keeps loaded counts within the authoritative total.
- **History accessibility**: the attempts section exposes busy state and politely announces loading plus the settled loaded/total count. Request errors remain assertive alerts with the existing keyboard retry action.
- **Actions**: cancellation is confirmed, available only through verifying, and hidden once `cancel_requested_at` is persisted. Retry requires `retryable=true` plus an exact Rust-supported failure code/stage pair; publication and remote-state ambiguity remain blocked regardless of contradictory metadata.
- **Confirmation**: the alertdialog starts on `Keep Task`, traps Tab and Shift+Tab between its two actions, and consumes Escape before restoring focus to `Cancel Task`. Escape closes the surrounding detail only when confirmation is absent.
- **Concurrency**: cancel and retry send the displayed version. HTTP 409 triggers exactly one selected-detail refetch plus one bounded current-page and count refresh before another action.
- **Responsiveness**: the inspector stacks beneath the table, wraps action controls, contains long technical values, and does not introduce document-level horizontal overflow.

### Connection Status
- **Structure**: indicator plus explicit lifecycle text and `/api/events` technical label.
- **States**: connecting before EventSource opens, connected after `open` or a valid event, reconnecting after a recoverable stream error, and unavailable after closure or missing EventSource support.
- **Accessibility**: status changes are announced politely and every state has explicit text; green is used only for a verified open stream.
- **Motion**: no decorative pulse.

### Worker Operations Surface
- **Structure**: a compact semantic table separates health, enabled policy, slot usage, task stages, transfer activity, last contact, and failure state; row actions open one shared native add/edit dialog.
- **Authority**: `GET /api/workers` is authoritative. Successful writes update from returned DTOs, while stale versions and retained SSE invalidations trigger one bounded list refetch.
- **Errors**: duplicate identity, busy deletion, stale version, invalid URL, and capacity conflicts remain visible beside the affected operation; field errors stay adjacent to matching controls, are programmatically associated, and move focus to the first invalid field.
- **Deletion confirmation**: the modal alertdialog names the exact worker and API URL, starts on `Keep Worker`, traps Tab and Shift+Tab between its two actions, and consumes Escape. Cancellation or rejection restores the invoking row action; successful deletion moves focus to the stable `Add Worker` action.
- **Responsiveness**: the table frame owns horizontal overflow and is keyboard-focusable; the route never creates document-level overflow.

### Runtime Settings Surface
- **Structure**: scheduler controls, transfer limits, timeout/retry fields, readiness, and restart-required configuration are separated by ruled sections rather than cards.
- **Authority**: runtime writes submit the displayed settings version. Stale writes refetch settings and readiness before another action.
- **Safety**: paths, cookie/session policy, and password-hash file location are read-only browser output. Credential material is never returned, entered, or stored.
- **Pause semantics**: pausing blocks new reservations, prefetch, and compute starts. Already-running processing continues; transfer and publication continue where applicable; cleanup continues.
- **Accessibility**: every numeric control has a visible label and exact server bounds; validation errors are programmatically associated and focus the first invalid field, including retry cross-field failures; readiness and mutation errors use explicit text rather than color alone.

## 6. Motion & Interaction

- `--motion-fast: 150ms cubic-bezier(0.16, 1, 0.3, 1)` for hover, focus, and active feedback.
- `--motion-enter: 240ms cubic-bezier(0.16, 1, 0.3, 1)` for the login and shell entrance.
- Only opacity and transform animate. Route changes do not use decorative transitions.
- `prefers-reduced-motion: reduce` removes entrance and active translations while retaining immediate state feedback.

## 7. Depth & Surface

Depth uses solid luminance stacking and fine structural borders rather than gradients or floating card shadows. The canvas is deepest, sidebar and login surfaces are one step brighter, and inputs/callouts are one step brighter again.

| Token | Value | Usage |
|---|---|---|
| `--radius-control` | `0.5rem` | Inputs and buttons |
| `--radius-panel` | `0.75rem` | Login and route callout |
| `--border-default` | `1px solid var(--color-border)` | Primary boundaries |
| `--border-subtle` | `1px solid var(--color-border-subtle)` | Quiet divisions |

## 8. Accessibility Constraints & Accepted Debt

Constraints:
- WCAG 2.2 AA contrast, visible keyboard focus, semantic landmarks, and ordered headings.
- Login, navigation, logout, bootstrap retry, and expiry recovery are keyboard operable.
- Manual task creation, task selection, detail dismissal, cancellation confirmation, and eligible retry are keyboard operable.
- Worker creation/editing, enable policy, deletion, scheduler pause/resume, and runtime settings updates are keyboard operable.
- No authentication material is written to local or session storage.
- No color-only status or error communication.
- No horizontal page overflow at 375px, 768px, or 1280px.
- Browser zoom, user font scaling, reduced motion, and color-scheme preferences remain usable.

Primary personas are a keyboard-first NAS operator, a low-vision operator using zoom/high contrast, and an operator recovering from an intermittent local-network failure.

Accepted debt:

| Item | Location | Why accepted | Owner / Exit |
|---|---|---|---|
| No client-side table virtualization | Tasks and Workers | Bounded operational datasets preserve semantic table navigation | Revisit only with measured scale evidence |
