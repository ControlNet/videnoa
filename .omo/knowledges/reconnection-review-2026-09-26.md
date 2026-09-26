# Review of Controller sleep reconnection fix

Reviewed commit: `58cdc11f82944f4fb04730c730a71d982660f5bd`.

## Assessment

No new actionable defect was confirmed in the reconnection changes. The new
CLOSED retry path addresses the previously missing application-managed recovery.
The implementation preserves native CONNECTING retries, caps application retry
delay at 30 seconds, resets backoff after open, gates additional recovery on
visibility/connectivity, and detaches old stream listeners before replacement.
Unmount removes its timers, stream, and browser lifecycle listeners.

## Existing issue that remains

The task detail stale-state finding from `project-review-2026-09-26.md` remains
unfixed and was not introduced by this commit. SessionEvents correctly publishes
`refetch` through `appInvalidationStore`, but `useTaskDetail` only subscribes to
`appTaskUpdateStore`. Reconnection can refresh the task list while leaving an
already-open detail pane at its pre-disconnection version. A missed terminal
transition may never receive another task delta to correct the pane.

The existing isolated regression probe was rerun against this commit. After
loading detail, publishing a reconnect invalidation, and rerendering, it still
failed with expected detail request count 2, actual count 1. Fixing this requires
the detail hook to consume global recovery invalidation, while preserving its
current content during refresh.

## Verification

```bash
npm --prefix controller-web test -- --run
npm --prefix controller-web run lint
npm --prefix controller-web run build
```

All commands exited zero: 179 tests across 29 files passed, including nine
SessionEvents recovery cases; lint and production build passed.

A temporary Chromium probe loaded the unmodified production SessionEvents
component with a synthetic local SSE backend. It used the browser's native
EventSource, not an EventSource substitute. The following assertions passed:

1. A first-request HTTP 503 produced CLOSED/unavailable, then the application
   retried successfully and received refetch invalidation.
2. Overlapping focus/online/pageshow/visibility signals did not replace an OPEN
   connection.
3. Ending an OPEN stream triggered native reconnect and a fresh refetch signal.
4. Unmount closed the live SSE connection.

Probe source: `/tmp/videnoa-reconnection-browser-probe.mjs`.
Probe output: `/tmp/videnoa-reconnection-browser-probe.log`.
Detail regression output: `/tmp/videnoa-reconnection-detail-probe.log`.
These are temporary local artifacts. The initial browser harness needed import
configuration corrections; the final four-assertion browser run exited zero.
No physical OS sleep, suspended-network silent OPEN condition, or deployed
reverse proxy behavior was reproduced. Those remain deployment verification
limits, not newly established defects. Application source was not changed during
this review.
