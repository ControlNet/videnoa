# Deep project review, third pass (2026-11-13)

Reviewed `dev` at `7bcc1a5`. Application code is unchanged. This pass adds two
findings to the eleven in the first two passes. The consolidated inventory is
`deep-review-consolidated-2026-11-13.md`.

## 12. P2: batched updates can lose an earlier task delta in the controller UI

`controller-web/src/events/taskUpdates.ts:15-21` retains only the latest task,
with a monotonically increasing generation. `tasks/useTasksData.ts:130-161`
processes that latest task and advances the applied generation without checking
whether intermediate generations were skipped. Distinct task updates that
arrive before the subscriber commits can therefore overwrite each other.

A React hook probe loaded two synthetic processing tasks at version 1, then
published version 2 of both in the same `act` batch. The second row advanced to
version 2; the first remained at version 1. The page request count stayed at 1,
so the hook did not repair the gap through a refetch. This can leave progress,
status, and available actions stale until another applicable event or reload.

This proves the hook/store behavior under batched delivery. It is not a browser
EventSource scheduling or end-to-end reproduction. The worker store has a
similar design, but only the task-list hook was exercised in this pass.

Fix: retain updates by entity ID or in a bounded event buffer, or invalidate and
refetch when generation gaps are detected. A generation counter alone does not
preserve the missing event contents.

## 13. P2: busy SSE streams can postpone overdue session authentication checks

`crates/controller/src/operations/events.rs:118-143` uses a biased select with
the event receiver before the 30-second authentication timer. When both are
ready, the receiver wins and emits an event. Continuous queued events can keep
the timer from being serviced, including after a session has been revoked.

A real router/body-stream probe established a cookie-authenticated SSE stream,
revoked its session, and separately confirmed that session authentication now
failed. It advanced Tokio's test clock by 31 seconds and queued a settings
change before each of three subsequent reads. All three reads returned
`scheduler_updated` events. Once the receiver emptied, the pending auth check
closed the stream. No real credentials or production settings were used.

An initial probe expected the next body chunk to be EOF immediately after the
three events; that assertion failed because SSE keepalive comments can precede
EOF. The final probe drains comments and checks that no additional data event is
emitted before closure. The three post-deadline data events occurred in both runs.

The observed consequence is continued event disclosure to a previously
authorized, now revoked connection. It does not establish a mutation-permission
bypass. The indefinite-delay consequence follows from the biased ordering when
events remain continuously ready; the bounded test exercised three events only.

Fix: prioritize an overdue auth check over event delivery, or compare an explicit
deadline before emitting each event. Preserve prompt shutdown handling.

## Historical findings rechecked, not newly discovered

- FFmpeg pipe reads/writes and child waits are synchronous and may block beyond
  cooperative cancellation. See `nodes/video_input.rs:672` and
  `nodes/video_output.rs:764` in core.
- Dynamic TensorRT engine creation/loading occurs at first inference, outside
  the session-creation cache lock and static-shape recovery path. See
  `crates/core/src/nodes/backend.rs:518-549`.
- Non-square source sample aspect ratio is not propagated; output forces
  `setsar=1`, potentially changing display proportions. See
  `crates/core/src/nodes/video_output.rs:374` and the October 23 project review.
- Worker endpoint edits are still allowed while attempts depend on that worker.
  `crates/controller/src/persistence/worker.rs:73-110` checks capacity but not
  endpoint immutability; `recovery/reconciler.rs:137-152` and
  `scheduler/download.rs:75-85` resolve the current endpoint for existing
  attempts. Old jobs can be polled or downloaded from another machine.
- The login limiter still counts failures only after verification:
  `crates/controller/src/auth/service.rs:164-182`. Concurrent requests can all
  pass before a failure is recorded, then queue on the two-permit password
  semaphore (`auth/password.rs:73-85`). There is no post-acquire limiter recheck
  or bounded waiter admission. This preserves the October 23 concurrency gap;
  no password burst or exhaustion test was run this pass.

The last two were explicitly left unfixed in the October 23 controller review.
Listing them here does not reverse that earlier scope decision.

## Candidates not treated as new defects

- Worker password rotation deliberately preserves existing tunnels; the
  September 7 iroh note records this policy. An established worker WebSocket
  remaining open is not the same issue as the controller SSE timer starvation.
- Controller uploads intentionally use current input contents, per the
  September 7 current-input-upload decision. Admission-time file identity is
  not a required execution invariant. README wording could clarify the scope.
- Debug replay values are pruned when a new job starts publishing values, so a
  simple claim of unbounded historical-job accumulation was not supported.

## Verification

The probes below use synthetic tasks/settings and the existing local auth test
fixture. They assert current defective behavior, not the expected fixed behavior.
The two temporary test files were removed after preserving their source here.

```sh
# Recreate each temporary file from the corresponding source block below first.
cargo test -p videnoa-controller --test review_third_probe -- --nocapture
npm --prefix controller-web test -- src/tasks/useTasksData.test.tsx src/tasks/review-third.test.tsx
# Existing regression tests, with no temporary files required:
cargo test -p videnoa-controller --test task14 sse:: -- --test-threads=1
```

- Final SSE probe: 1 passed, 0 failed, 1.25 seconds.
- UI probe plus existing useTasksData tests: 4 passed across 2 files, 4.43 seconds.
- Existing SSE regression tests: 6 passed, 0 failed, 48.22 seconds.
- No application code was changed. No GPU workload, production traffic, real
  browser E2E, hard-stop fault injection, or memory exhaustion was exercised.

## Synthetic probe sources

### crates/controller/tests/review_third_probe.rs

```rust
#[path = "task14/support.rs"]
mod support;

use axum::http::header;
use futures_util::StreamExt;
use tower::ServiceExt;
use videnoa_controller::{auth::{AuthService, SESSION_COOKIE}, config::ControllerConfig, domain::SecretString, persistence::SettingsUpdate};

#[tokio::test]
async fn revoked_sse_session_drains_events_before_overdue_auth_check() -> support::TestResult {
    let fixture = support::Fixture::new().await?;
    let config = ControllerConfig::for_workspace(&fixture.workspace)?;
    let auth = AuthService::new(config.auth, fixture.store.clone())?;
    let session = auth.login("127.0.0.1".parse()?, &SecretString::new(support::PASSWORD), chrono::Utc::now()).await?;
    let mut request = support::Fixture::request("GET", "/api/events", None)?;
    request.headers_mut().remove(header::AUTHORIZATION);
    request.headers_mut().insert(header::COOKIE, format!("{SESSION_COOKIE}={}", session.token).parse()?);
    let response = fixture.router.clone().oneshot(request).await?;
    assert_eq!(response.status(), 200);
    let mut stream = response.into_body().into_data_stream();
    let _ = stream.next().await.ok_or("initial event missing")??;
    fixture.store.revoke_session(session.response.session.id, chrono::Utc::now()).await?;
    assert!(auth.authenticate_session_at(&session.token, chrono::Utc::now()).await.is_err());
    tokio::time::pause();
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    for _ in 0..3 {
        let settings = fixture.store.config_manager().settings()?;
        let mut scheduler = settings.scheduler;
        scheduler.paused = !scheduler.paused;
        fixture.scheduler.update_settings(SettingsUpdate { expected_version:settings.version, scheduler, timeouts:settings.timeouts, retry:settings.retry, updated_at:chrono::Utc::now() }).await?;
        let event = stream.next().await.ok_or("stream closed")??;
        assert!(String::from_utf8_lossy(&event).contains("scheduler_updated"));
    }
    println!("revoked session received 3 deltas after the 30s auth deadline");
    // SSE keepalive comments may precede the eventual end of the body.
    while let Some(event) = stream.next().await {
        let event = event?;
        assert!(!String::from_utf8_lossy(&event).contains("scheduler_updated"));
    }
    Ok(())
}
```

### controller-web/src/tasks/review-third.test.tsx

```tsx
import { act, renderHook, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { appTaskUpdateStore } from "../events/taskUpdates"
import { createApiClient } from "../api/client"
import { parseTaskQuery } from "./query"
import { useTasksData } from "./useTasksData"

const initialTask = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  version: 1,
  status: "processing",
  input_path: "/media/old-query.mkv",
  output_path: "/output/old-query.mp4",
  input_extension: "mkv",
  output_extension: "mp4",
  workflow: "anime-2x",
  priority: 1,
  source: "manual",
  source_reference: null,
  input_size: 1024,
  worker_id: "550e8400-e29b-41d4-a716-446655440001",
  requested_worker_id: null,
  remote_job_id: null,
  progress: {
    percent: 30,
    processed_frames: 300,
    total_frames: 1000,
    frames_per_second: 24,
    eta_seconds: 30,
    bytes_transferred: null,
    bytes_total: null,
  },
  attempt_count: 1,
  failure: null,
  cancel_requested_at: null,
  created_at: "2030-01-01T00:00:00Z",
  updated_at: "2030-01-01T00:01:00Z",
  completed_at: null,
  allowed_actions: [] as string[],
} as const

const emptyCounts = {
  items: [
    "queued", "reserved", "uploading", "staged", "submitting", "processing", "remote_completed",
    "downloading", "verifying", "publishing", "remote_cleanup", "completed", "failed", "cancelled",
  ].map((status) => ({ status, count: 0 })),
  total: 0,
}


it("review: a burst loses the earlier task update", async () => {
  const second = { ...initialTask, id: "550e8400-e29b-41d4-a716-446655440002" };
  let reads = 0;
  const fetcher: typeof fetch = async (input, init) => {
    if (new URL(new Request(input, init).url).pathname === "/api/status-counts") return Response.json(emptyCounts);
    reads++;
    return Response.json({ items: [initialTask, second], total: 2, limit: 50, offset: 0 });
  };
  const apiClient = createApiClient({ fetcher, onUnauthorized: () => undefined });
  const query = parseTaskQuery(new URLSearchParams());
  const { result, unmount } = renderHook(() => useTasksData(apiClient, query));
  await waitFor(() => expect(result.current.loading).toBe(false));
  act(() => {
    appTaskUpdateStore.publish({ ...initialTask, version: 2, progress: { ...initialTask.progress, percent: 60 } });
    appTaskUpdateStore.publish({ ...second, version: 2, progress: { ...second.progress, percent: 70 } });
  });
  await waitFor(() => expect(result.current.page?.items[1]?.version).toBe(2));
  expect(result.current.page?.items[0]?.version).toBe(1);
  expect(reads).toBe(1);
  console.log("Burst update versions:", result.current.page?.items.map(task => task.version));
  unmount();
});
```
