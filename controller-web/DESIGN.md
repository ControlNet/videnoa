# Videnoa Controller Design System

## 1. Atmosphere & Identity

Videnoa Controller is a quiet operational surface for coordinating video-processing services. It inherits Videnoa's cool neutral palette, violet focus color, compact geometry, Manrope body text, and Geist Mono product mark. The signature is a single live service line that makes the Rust boundary visible without introducing later-task dashboards or controls.

## 2. Color

| Role | Token | Light | Dark | Usage |
|---|---|---|---|---|
| Background | `--color-background` | `oklch(0.98 0.005 260)` | `oklch(0.13 0.028 260)` | Page canvas |
| Surface | `--color-surface` | `oklch(1 0 0)` | `oklch(0.16 0.028 260)` | Shell panel |
| Text | `--color-text` | `oklch(0.15 0.02 260)` | `oklch(0.97 0.005 260)` | Primary copy |
| Muted text | `--color-text-muted` | `oklch(0.45 0.015 260)` | `oklch(0.7 0.015 260)` | Supporting copy |
| Border | `--color-border` | `oklch(0.88 0.01 260)` | `oklch(0.28 0.02 260)` | Structural separation |
| Accent | `--color-accent` | `oklch(0.55 0.22 292)` | `oklch(0.58 0.22 292)` | Product mark and focus |
| Accent border | `--color-accent-border` | Accent mixed with 55% border | Accent mixed with 55% border | Product mark outline |
| Healthy | `--color-healthy` | `oklch(0.54 0.16 145)` | `oklch(0.72 0.17 145)` | Live service status |

Rules:
- Colors are defined only as custom properties in `src/index.css`.
- Violet is reserved for identity and focus; green is reserved for real healthy state.
- The shell follows the user's color-scheme preference without a task-specific theme control.

## 3. Typography

| Level | Token | Size | Weight | Line height | Usage |
|---|---|---|---|---|---|
| Product | `--type-product` | `0.875rem` | 600 | 1.4 | Product mark |
| Heading | `--type-heading` | `clamp(2rem, 5vw, 4rem)` | 600 | 1.05 | Shell title |
| Body | `--type-body` | `1rem` | 400 | 1.6 | Description |
| Label | `--type-label` | `0.75rem` | 600 | 1.4 | Status metadata |

Font stacks:
- Body: `Manrope`, `Avenir Next`, `Segoe UI`, sans-serif.
- Mono: `Geist Mono`, `SFMono-Regular`, `Consolas`, monospace.

## 4. Spacing & Layout

The base unit is 4px.

| Token | Value | Usage |
|---|---|---|
| `--space-1` | `0.25rem` | Tight indicator spacing |
| `--space-2` | `0.5rem` | Inline clusters |
| `--space-3` | `0.75rem` | Compact groups |
| `--space-4` | `1rem` | Mobile page gutter |
| `--space-6` | `1.5rem` | Panel padding |
| `--space-8` | `2rem` | Major group gap |
| `--space-12` | `3rem` | Desktop page gutter |

The root is a `cover` shell bounded by `100dvb`. Its main region owns vertical scrolling. Content is limited to `72rem`, uses one column, and reflows without horizontal scrolling at 375px.

## 5. Components

### Controller Shell
- **Structure**: `main` cover, product header, one content section, service footer.
- **Spacing**: `--space-4`, `--space-6`, `--space-8`, `--space-12`.
- **States**: static operational shell; no loading or interactive state is introduced in Task 1.
- **Accessibility**: one `h1`, labelled service status, readable source-order at every width.
- **Motion**: none; Task 1 has no state transition that earns animation.
- **Layout**: bounded `cover`; the page body is the only scroll owner.

### Service Status
- **Structure**: semantic status text with one indicator and endpoint label.
- **States**: healthy only, because the shell is served by the live Controller process. Runtime health polling belongs to later tasks.
- **Accessibility**: color is supplemented by explicit `Service online` text.
- **Motion**: none; no decorative pulse.

## 6. Motion & Interaction

- Transition token: `--motion-fast: 150ms ease-out`, reserved for future focus/hover states.
- Task 1 introduces no clickable controls and no automatic animation.
- `prefers-reduced-motion` remains respected by shipping no non-essential motion.

## 7. Depth & Surface

Strategy: mixed tonal shift plus one structural border. The page background and shell surface create hierarchy; the shell uses `--color-border` and no decorative shadow.

| Token | Value | Usage |
|---|---|---|
| `--radius-shell` | `0.75rem` | Main shell panel |
| `--border-default` | `1px solid var(--color-border)` | Shell boundary |

## 8. Accessibility Constraints & Accepted Debt

Constraints:
- WCAG 2.2 AA contrast target.
- Semantic landmarks and heading order.
- No color-only status communication.
- No horizontal overflow at 375px, 768px, or 1280px.
- Browser zoom and user font scaling must remain usable.

Accepted debt:

| Item | Location | Why accepted | Owner / Exit |
|---|---|---|---|
| No authenticated navigation or domain controls | Entire shell | Those belong to Tasks 15-18, not Task 1 | Implement under their dedicated plan tasks |
