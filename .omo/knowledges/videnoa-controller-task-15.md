# Videnoa Controller Task 15

## Authenticated browser boundary

- The browser client uses same-origin HttpOnly cookie authentication and keeps the rotated CSRF proof only in the `ApiClient` closure. Password, session, Bearer material, and CSRF proof are never persisted in browser storage.
- Session bootstrap parses every response through Zod. The client normalizes flat authentication errors and nested operations envelopes into one typed `ApiClientError`, while rejecting malformed success and error payloads as `malformed_response`.
- A captured browser-native `fetch` must retain the browser global receiver. Calling it as an options-object method changes `this` and Chromium throws `Illegal invocation` before an HTTP request exists. `Reflect.apply(fetcher, globalThis, [request])` preserves both dependency injection and native browser behavior.

## Shell and accessibility

- The authenticated shell owns protected Tasks, Workers, and Settings routing, route-main focus, logout, and the authenticated SSE connector. Route bodies intentionally remain readiness placeholders: Tasks belongs to Task 16, while Workers and Settings belong to Task 18.
- Responsive CSS may remove visible button text, but the control must retain an explicit accessible name when its icon is `aria-hidden`. The narrow sign-out button therefore carries a state-aware `aria-label`.
- The fixed shell uses `100dvb`, a bounded grid, and `.shell-main` as the only scroll owner. Visual captures should wait for `document.getAnimations()` to settle before measuring root geometry because the entrance transform can transiently add one pixel to full-page bounds.
- Recoverable logout errors return a typed result instead of rejecting into the event loop. The shell retains authentication, restores the sign-out control, and focuses an alert so the operator can retry.
- EventSource lifecycle is user-visible state: connecting is the initial mount, `open` or `refetch` proves connected, a non-closed error means reconnecting, and a closed or unsupported stream means unavailable. The mandatory initial `refetch` remains an invalidation signal, not an error.
- Responsive layouts must preserve that explicit lifecycle text. Below 48rem the compact shell hides only the secondary `/api/events` label; removing the whole status would violate the no-color-only communication contract.
- A root React error boundary contains unexpected render failures and focuses a retry action that remounts the application tree without persisting or exposing authentication material.
- Route-main focus is a pre-paint shell contract. Use `useLayoutEffect` for the pathname/title/focus transition so committed authenticated content cannot become observable while focus is still on `body`; passive `useEffect` made the immediate bootstrap assertion suite-timing-sensitive.

## Verification

- Vitest covers both API error envelopes, malformed boundary rejection, CSRF rotation/attachment/clearing, fetch receiver behavior, render recovery, auth recovery, protected routing, logout retry, connection lifecycle, and invalidation.
- Playwright covers login, navigation, ownership labels, reload restoration, narrow layout, empty auth storage, successful and failed logout, wrong password, malformed response, network failure, and expiry focus recovery.
- Existing-session reload coverage must assert that the authenticated main landmark is focused, not merely that its route content is visible.
- Production build, lint, typecheck, release embedded-SPA routing, changed-file diagnostics, and an exact fresh visual evidence set remain required before Task 15 can be independently accepted. Stale or extra screenshots invalidate that evidence set even when current captures pass.
- The canonical visual matrix is 14 CSS-pixel viewport captures: login, Tasks, Workers, and Settings at 375x812, 768x900, and 1280x900, plus logout-error at 375x812 and 1280x900. Capture with an explicit color scheme, reduced motion, settled animations, and `fullPage: false`.
- Task 15 permits no rendered gradients. Depth comes from semantic solid luminance layers, borders, accent edges, and inset highlights.
- Programmatically focused alerts need an explicit `:focus` outline so the accessibility state is visible in static evidence rather than inferred from DOM assertions alone.
- Final visual acceptance requires two independent reviewers to inspect the complete fresh matrix; both the UX/accessibility and strict source-integrity passes approved this remediation with no blockers.

## Logout focus priority remediation

- Route focus and recoverable-error focus must not be independent effects with different scheduling phases. If a logout alert remains mounted during a route transition, the pathname layout effect can steal focus without retriggering an alert effect keyed only to the error value.
- The shell now resolves one explicit focus owner during route commits: the logout alert when present, otherwise the main landmark. Logout-error focus also runs in the layout phase so the committed accessibility state is observable before paint.
- Regression coverage must force the conflict rather than only assert initial alert focus: fail logout, navigate to another protected route while the alert remains mounted, assert the alert retains focus, retry logout, and verify successful return to login.
