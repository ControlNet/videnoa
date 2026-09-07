# Videnoa Controller Task 21

## Adversarial Test Composition

- Dedicated integration targets can re-export predecessor test modules with `#[path]` or `include!` while preserving their private fixtures and avoiding production edits.
- Barrier-controlled mixed-body intake is stronger than separate replay/conflict tests because it proves creator election and canonical-body ownership under one simultaneous key race.
- Duplicate publication calls are safe only when evidence proves exactly one completed outcome, exact destination bytes, and a completed durable task rather than merely accepting two settled futures.

## Load Boundary Resolution

- Task detail now reuses `PageRequest`: 100 attempts by default, a hard maximum of 500, and explicit `total`, `limit`, and `offset` metadata. The measured 500-attempt offset page is 315,233 bytes.
- Attempt pages use stable `attempt_no DESC, id DESC` ordering. A 20,000-attempt task now returns a 63,623-byte default detail response instead of the previous 12,569,348-byte response.
- The frontend requests the newest 100 attempts first and appends older pages only when the operator activates `Load more attempts`.

## Security Evidence Rules

- Browser security evidence should inspect storage keys and cookie names only; never serialize cookie, CSRF, bearer, or password values.
- Task-specific Playwright configuration prevents a focused security run from mutating prior task evidence and keeps trace/video disabled.
- Cross-filesystem proof must distinguish executed different-device policy from injected EXDEV and from native Windows execution.

## Frontend History Ownership

- Abort alone is not a stale-response guarantee. History completion must match the active request object, selected task ID, detail generation, and requested offset before it can mutate detail, loading, or error state.
- Store the active detail and request owner in refs so repeated activations are rejected synchronously before React state commits a disabled button.
- Append pages by filtering already-rendered attempt IDs, preserving server order, and slicing to the authoritative total.
- Production-preview race tests should strip the history request signal when proving ownership checks, because otherwise transport cancellation can hide stale completion bugs.

## Visual CJK Evidence

- The Linux Playwright host provides Japanese and Chinese glyphs through `Droid Sans Fallback` but no Korean font according to Fontconfig.
- Task Detail names common Noto, Apple, Microsoft, and Droid CJK fallbacks; deterministic screenshots should use glyph coverage actually installed on the capture host rather than accepting tofu as a product result.
