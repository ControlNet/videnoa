# Videnoa Controller Design System

## 1. Atmosphere & Identity

Videnoa Controller is a compact industrial control surface for operators coordinating durable video-processing work. It uses Videnoa's Manrope and Geist Mono typography, cool graphite layers, a restrained violet focus accent, and one green live-state signal.

The governing decision is that this is an operations console, not a document. A route announces itself in one 48px command row that also carries its primary action; there is no display-type page header, no eyebrow label and no route description paragraph. Chrome is measured in rows, and every row spent on chrome is a row not spent on work.

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
| Healthy wash | `--color-healthy-wash` | `oklch(0.94 0.025 145)` | `oklch(0.2 0.04 145)` | Saved and applied confirmation |
| Danger | `--color-danger` | `oklch(0.48 0.18 25)` | `oklch(0.7 0.17 25)` | Recoverable error text |
| Danger wash | `--color-danger-wash` | `oklch(0.94 0.025 25)` | `oklch(0.2 0.04 25)` | Error summary |

Every state in the product resolves to one of four semantic tones. Fourteen task statuses, worker health, scheduler state and readiness all draw from this set, so a dense table reads as four signals rather than fourteen coloured blocks. The exact state always stays in the text beside the dot.

| Tone | Token | Source | Meaning |
|---|---|---|---|
| Quiet | `--tone-quiet` | `--color-text-quiet` | Waiting or settled without outcome (`queued`, `cancelled`) |
| Active | `--tone-active` | `--color-accent` | In flight (every stage between `reserved` and `remote_cleanup`) |
| Positive | `--tone-positive` | `--color-healthy` | Verified success (`completed`, worker online, scheduler running) |
| Negative | `--tone-negative` | `--color-danger` | Failure requiring attention (`failed`, worker offline) |

Rules:
- All color values are declared as semantic custom properties in `src/index.css`.
- Status is never communicated by a tone alone: `Status` renders a dot plus its exact label.
- Violet is reserved for identity, focus, active navigation, and primary actions.
- Green is reserved for verified connected state. Error red is always paired with explicit text.
- The application follows `prefers-color-scheme`; Task 15 adds no stored theme preference.

## 3. Typography

The ramp is operational. The largest type in the authenticated product is a 15px route title; display sizes exist only on the isolated authentication panel.

| Level | Token | Size | Weight | Usage |
|---|---|---|---|---|
| Login title | `--type-login-title` | `1.25rem` | 600 | Sign-in and setup heading |
| Route title | `--type-page` | `0.9375rem` | 600 | Command-row route heading, dialog titles |
| Section | `--type-section` | `0.8125rem` | 600 | Section headings |
| Body | `--type-body` | `0.8125rem` | 400-500 | Navigation, descriptions |
| Cell | `--type-cell` | `0.78125rem` | 400-500 | Table cells, field values |
| UI | `--type-ui` | `0.75rem` | 600 | Buttons and controls |
| Chip | `--type-chip` | `0.6875rem` | 500-600 | Filter chips, metadata, footnotes |
| Micro | `--type-micro` | `0.59375rem` | 600 | Column headers, key labels |
| Metric | `--type-metric` | `1.0625rem` | 600 | Instrument values in the inspector |

Font stacks:
- Body and display: `Manrope`, `Avenir Next`, `Segoe UI`, sans-serif.
- Product and technical labels: `--font-mono` (`Geist Mono`, `SFMono-Regular`, `Consolas`, plus Noto Sans Mono CJK fallbacks), with `font-variant-numeric: tabular-nums` on every numeric column.
- Titles use tight negative tracking; column and key labels use uppercase with `0.08-0.09em` tracking.

## 4. Spacing & Layout

The base unit is 4px. Tokens are `--space-1` through `--space-12` at 4, 8, 12, 16, 20, 24, 32, and 48px.

Structure is expressed as named measures rather than ad-hoc values:

| Token | Value | Usage |
|---|---|---|
| `--rail-width` | `12.5rem` | Desktop navigation rail |
| `--row-command` | `3rem` | Route command row |
| `--row-filter` | `2.625rem` | Filter and counter band |
| `--row-foot` | `2.5rem` | Pagination footer |
| `--table-head` / `--table-row` | `1.875rem` / `2.125rem` | Dense table measures |
| `--control-md` / `--control-sm` | `1.875rem` / `1.625rem` | Buttons and chips |
| `--control-field` | `2rem` | Form inputs |
| `--drawer-width` | `35rem` | Task inspector |
| `--gutter` | `1.125rem` | Route inline gutter |

The authenticated root is an `app-frame` grid bounded by `100dvb`. It takes a definite block size so implicit rows cannot grow to intrinsic route height, which keeps `.shell-main` the only vertical scroll owner of the shell and pins the rail and sign-out control.

Route sizing is opted into by each page rather than inherited, because stylesheet order must not decide which rule wins:
- `.route-page.tasks-page` is capped at the shell height and scrolls internally, so its command row, filters and pagination are always on screen.
- `.route-page.operation-page` grows and lets `.shell-main` scroll.

At widths below 48rem the frame becomes rows: a compact top bar carrying identity and connection state, the route body, and a 56px navigation tab bar with 44px+ hit targets. Exactly one navigation is rendered at a time -- a duplicate set hidden only by CSS would still reach the accessibility tree.

## 5. Components

Routes compose from one shared vocabulary in `src/ui/`. A route-specific rule that restates a primitive -- another button, another field frame, another dense table -- is a defect, not a variation.

### Primitives

- **Button** (`ui/Button.tsx`): `primary`, `outline`, `ghost`, `danger` variants at `md`/`sm`, plus a square `icon` form that requires an explicit label. Active state translates 1px; reduced motion removes it.
- **Field** (`ui/Field.tsx`): visible label, control, and programmatically associated error. `CheckField` covers boolean policy. No placeholder-as-label anywhere in the product, and nothing but the label contributes to a control's accessible name.
- **Chip** (`ui/Chip.tsx`): `SelectChip` and `TextChip` wrap a native `select`/`input` in compact chrome so keyboard behaviour, form semantics and assistive-technology reporting stay the platform's. An engaged filter is shown by accent border and wash. `shortLabel` shortens visible text without changing the accessible name.
- **Status** (`ui/Status.tsx`): dot plus exact label in one of four tones. The live halo is reserved for a verified open stream.
- **Scroll frame** (`.scroll-frame` + `.scroll-controls`): the named overflow region and its boundary-aware controls.
- **Data table** (`.data-table`): sticky header, dense rows, `grow-cell` for the one column that absorbs remaining width.
- **useMediaQuery** (`ui/useMediaQuery.ts`): drives structural changes that must not be expressed as duplicated markup.

### Authentication Bootstrap
- **Sequence**: startup checks initialization before checking the cookie session. An uninitialized Controller renders setup; an initialized Controller proceeds to session recovery and sign-in.
- **Race recovery**: a setup conflict means another client initialized the Controller. The client rechecks initialization and session state, then presents normal sign-in without retaining the submitted values.
- **Security**: passwords exist only in component and request memory. Setup, login, logout, reload, and expiry recovery never write authentication material to browser storage.

### Setup Panel
- **Structure**: product mark, first-run purpose statement, labelled password and confirmation fields, inline validation or recovery summary, primary setup action.
- **States**: idle, submitting, mismatched confirmation, password outside the 12-1024 UTF-8 byte boundary, already-initialized race, malformed response, and network failure.
- **Accessibility**: password receives initial focus; field errors are programmatically associated; the first invalid field or recovery summary receives focus.

### Login Panel
- **Structure**: product mark, purpose statement, labelled password field, inline error summary, primary submit.
- **States**: idle, submitting, wrong password, malformed response, network failure, rate limited.
- **Accessibility**: password receives initial focus; errors use `role="alert"` and receive programmatic focus; submit state is announced through its label.
- **Security**: password exists only in component and request memory and is cleared after success.

### Application Frame
- **Structure**: product identity, primary navigation, service state, sign-out action, scrolling route main.
- **States**: authenticated, recoverable sign-out failure, and session-expired. A failed sign-out keeps the authenticated frame mounted and focuses a retryable alert; expiry replaces the entire frame with login.
- **Accessibility**: labelled primary navigation; current route uses `aria-current="page"`; route changes focus the main landmark.
- **Layout**: a 200px rail at desktop carrying a compact product mark, three navigation items, connection state and sign-out. Below 48rem the rail becomes a top identity bar and navigation moves to a bottom tab bar; exactly one navigation is mounted at a time.

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

- **Structure**: a 48px command row (route title, path search, column picker, `Add Task`) above a 42px band carrying status counters and the filter chips, then the table, then a pinned pagination footer. Total route chrome is 90px; the route is exactly one viewport tall and never scrolls the shell.
- **States**: loading rows, populated page, empty filter result, recoverable load failure, and live active-row replacement.
- **Density**: 30px header, 34px rows, 12.5px cell type. Row separators replace cards; numeric and identifier cells use Geist Mono with tabular numerals; long values truncate with native title disclosure.
- **Status column**: one dot plus the exact status label in its semantic tone, so column width no longer tracks the longest status name.
- **Dates**: `Created` renders relative time with the absolute timestamp in `title`. Values older than thirty days, and any future timestamp, fall back to the absolute format.
- **Columns**: Status, Name, Workflow, Worker, Progress, ETA, Size and Created are the default set, chosen so a 1440px viewport has no inline overflow. FPS lives in the inspector. Input Path, Output Path, Attempts, Duration, Failure Stage, Failure, Error and Remote Job ID remain independent URL-persisted options. A generic Path option is not used because it obscures whether the value is an input or output path.
- **Responsiveness**: the route never owns horizontal overflow; the table frame is the deliberate scroll region in both axes. A concise associated hint and compact boundary-aware navigation remain visible above the frame while overflow exists at any viewport width, and the named frame becomes a visible-focus keyboard scroll stop. Below 62rem the counters and the filter chips each become one horizontally scrollable row rather than stacking.
- **Live data**: matching active task deltas replace only newer row versions; membership or ordering changes refetch the bounded current page and counts.

### Manual Task Intake
- **Structure**: a compact native modal with exact input/output paths, workflow, integer priority, and explicit manual source semantics.
- **Idempotency**: the client keeps one in-memory UUID only for an unchanged request whose response was lost. Any field edit or confirmed API response ends that intent; a changed submission receives a new key.
- **Errors**: boundary validation uses the closed server field-error code set, preserves adjacent messages, and moves focus to the first invalid control. Structured path messages drive outside-root and no-clobber guidance even when the top-level message is generic. Ambiguous transport failure offers an explicit `Retry Same Task`; key/body conflicts explain that the next submission is a new intent.
- **Accessibility**: native modal focus containment, Escape dismissal, visible labels, and trigger-focus restoration support keyboard-only operation.

### Task Detail Inspector

- **Structure**: selecting a task opens a drawer over the right of the table region, not a pane beneath it. The row that was selected stays where the operator left it, and the command row, filters and pagination remain reachable while the drawer is open.
- **Layout**: the drawer owns its own vertical scroll, its header is sticky, and its content keeps safe bottom padding so the final section is never clipped. Progress renders as instrument tiles rather than another label/value list. Below 48rem the drawer takes the full route width.
- **Stacking**: the command row out-stacks the drawer so the column picker stays operable while a task is open.
- **Authority**: list rows and SSE deltas select or invalidate; bounded `GET /api/tasks/{id}?limit=&offset=` pages remain the source of truth for versions, attempts, and action eligibility. The inspector loads 100 newest attempts first and exposes an explicit next-page action while more persisted history exists.
- **History concurrency**: each next-page request is owned by the selected task, detail generation, and requested offset. Selection changes, manual reloads, and SSE invalidation abort pending history work; ownership checks reject late transport completions, while ID deduplication preserves newest-to-oldest order and keeps loaded counts within the authoritative total.
- **History accessibility**: the attempts section exposes busy state and politely announces loading plus the settled loaded/total count. Request errors remain assertive alerts with the existing keyboard retry action.
- **Actions**: cancellation is confirmed, available only through verifying, and hidden once `cancel_requested_at` is persisted. Retry requires `retryable=true` plus an exact Rust-supported failure code/stage pair; publication and remote-state ambiguity remain blocked regardless of contradictory metadata.
- **Confirmation**: the alertdialog starts on `Keep Task`, traps Tab and Shift+Tab between its two actions, and consumes Escape before restoring focus to `Cancel Task`. Its actions carry an explicit focus ring because focus arrives programmatically. Escape closes the surrounding detail only when confirmation is absent.
- **Concurrency**: cancel and retry send the displayed version. HTTP 409 triggers exactly one selected-detail refetch plus one bounded current-page and count refresh before another action.

### Connection Status
- **Structure**: indicator plus explicit lifecycle text and `/api/events` technical label.
- **States**: connecting before EventSource opens, connected after `open` or a valid event, reconnecting after a recoverable stream error, and unavailable after closure or missing EventSource support.
- **Accessibility**: status changes are announced politely and every state has explicit text; green is used only for a verified open stream.
- **Motion**: no decorative pulse.

### Worker Operations Surface

- **Structure**: a 48px command row carrying the route title, a registered/slots-busy summary and `Add Worker`, above a dense semantic table. Capacity leads: each row shows used/total slots as a value and as one pip per compute slot, alongside health, enabled policy, task stages, transfer activity, last contact and failure state. Row actions are always visible and open one shared native add/edit dialog.
- **Health**: the row carries a tone edge and a `Status` dot; online health and enabled scheduling policy remain independent, separately labelled states.
- **Authority**: `GET /api/workers` is authoritative. Successful writes update from returned DTOs, while stale versions and retained SSE invalidations trigger one bounded list refetch.
- **Errors**: duplicate identity, busy deletion, stale version, invalid URL, and capacity conflicts remain visible beside the affected operation; field errors stay adjacent to matching controls, are programmatically associated, and move focus to the first invalid field.
- **Deletion confirmation**: the modal alertdialog names the exact worker and API URL, starts on `Keep Worker`, traps Tab and Shift+Tab between its two actions, and consumes Escape. Cancellation or rejection restores the invoking row action; successful deletion moves focus to the stable `Add Worker` action.
- **Responsiveness**: the table fits a desktop viewport without inline overflow. Below 48rem the table frame owns horizontal overflow, exposes keyboard-operable navigation controls, and is itself focusable; the route never creates document overflow.

### Runtime Settings Surface
- **Structure**: a 48px command row carries the route title, the settings version, the configuration file path and the scheduler state pill with its pause/resume action. A sticky section index sits beside the form; server binding, authentication policy, scheduler capacity, timeouts and retry policy are ruled sections whose fields flow in a responsive multi-column grid. Readiness and safe path context follow.
- **Commit visibility**: the save bar is the route's last child and is sticky to the viewport bottom, so a long form never hides its own commit. The submit control is associated with the form by its `form` attribute rather than by containment.
- **Authority**: runtime writes submit the displayed settings version. Stale writes and retryable committed-degraded responses refetch settings and readiness before another action.
- **Persistence**: a successful save explicitly confirms that the configuration file was written and the returned settings were applied to the running Controller.
- **Reconnect**: when host or port changes, the UI exposes a usable Controller link based on the effective configured endpoint and current browser host for wildcard bindings. Successful saves place it in the receipt; committed-degraded saves keep it with the exact failure alert and never claim that configuration projection succeeded.
- **Safety**: workspace, data root, and configuration file are read-only context. Fixed input/output roots, temporary roots, password-hash paths, and credential material are never returned, entered, shown, or stored.
- **Pause semantics**: pausing blocks new reservations, prefetch, and compute starts. Already-running processing continues; transfer and publication continue where applicable; cleanup continues.
- **Accessibility**: every public server, authentication, scheduler, timeout, and retry field is editable through a visible labelled control. Validation errors are programmatically associated and focus the first invalid field, including retry cross-field failures; readiness, mutation errors, and save receipts use explicit text rather than color alone.

### Configuration Save Receipt
- **Structure**: saved-and-applied status, configuration file path, and an optional reconnect link when server binding changed.
- **States**: saved in place and saved with endpoint change.
- **Accessibility**: uses status semantics, meaningful link text, and explicit connection guidance without relying on green alone.

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
- Setup, login, navigation, logout, bootstrap retry, and expiry recovery are keyboard operable.
- Manual task creation, task selection, detail dismissal, cancellation confirmation, and eligible retry are keyboard operable.
- Worker creation/editing, enable policy, deletion, scheduler pause/resume, and runtime settings updates are keyboard operable.
- No authentication material is written to local or session storage, and no password-hash path is rendered.
- No color-only status or error communication.
- No horizontal page overflow at 375px, 768px, or 1280px.
- Browser zoom, user font scaling, reduced motion, and color-scheme preferences remain usable.

Primary personas are a keyboard-first NAS operator, a low-vision operator using zoom/high contrast, and an operator recovering from an intermittent local-network failure.

Accepted debt:

| Item | Location | Why accepted | Owner / Exit |
|---|---|---|---|
| No client-side table virtualization | Tasks and Workers | Bounded operational datasets preserve semantic table navigation | Revisit only with measured scale evidence |
| Webfonts load from Google Fonts over the network | `index.html` | Deferred by explicit product decision during the 2026-09-06 redesign | Bundle Manrope and Geist Mono, and add a Hangul/CJK-capable fallback. Until then a Controller on an offline LAN renders the fallback stack, and the F3 Hangul rendering defect stays open |
| Filter chips are horizontally scrollable below 62rem | Tasks | Keeps every server-backed filter reachable and testable in one row instead of a five-row stack | Revisit if a filter disclosure is preferred over inline scrolling |
