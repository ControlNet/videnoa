# Controller submission stall diagnosis

## Live observations

- Controller API reported task `ef500dc7-3d3d-4233-bcfa-86d8b7debd15` in `submitting`, with no remote job ID, failure, or scheduled retry. Attempt count remained one.
- Worker API reported job `36f50208-ec4e-4d13-8081-821fe77585b7` running. Its workspace path contains that exact controller task ID; progress was 41,515 / 102,141 frames during inspection.
- Logs show submission started at 2026-09-10 05:14:48 UTC. Worker job creation was at 05:14:48.988 UTC and its running transition at 05:15:00.285 UTC.
- Assigned worker gpu4 was online. Repeated health failures concerned the different worker gpu3 using Iroh; they do not establish a failure of the assigned HTTP worker.
- Live controller identified itself as 0.1.3; local checkout is 0.1.4. The API does not expose a build commit, so exact deployed source provenance is unverified.
- Live settings had `poll_seconds = 5`. Current code also uses this as the total remote control-request timeout, including submission.

## Recovery behavior in source

`recovery/submission.rs` claims durable submission ownership before issuing `/api/run`. `persistence/submission_claim.rs` refuses another claim by the same reconciler generation. An owned attempt is deferred without retrying the request. Only a new generation can reclaim it.

Transient submission errors return without clearing ownership or storing retry metadata. The orchestration loop logs retryable advance errors at DEBUG. Thus an accepted request with a lost/timed-out response can leave the task indefinitely submitting with no INFO/WARN failure message. An existing test, `submission_ownership::timed_out_submission_waits_for_restart_before_replay`, explicitly covers acceptance with a dropped response and recovery after restart.

The original failing response was not captured in the supplied logs. A timeout is plausible, but the live evidence alone cannot distinguish transport failure from another failure before receipt persistence. The confirmed defect is the lack of recovery within the same generation after an interrupted submission.

## Recovery and follow-up

Restarting only the controller with its existing database allows a new generation to replay the same durable idempotency identity. The worker's persisted mapping is intended to return the existing job instead of creating another. This was not performed on the live services during diagnosis. Do not delete/recreate the task or restart the worker to resolve this display state.

A durable fix should allow bounded reconciliation after the request has ended, while preventing concurrent submissions and preserving the same idempotency identity. Successful receipts should be retained/reconciled if their first database write conflicts. Deferred submission failures should be observable at WARN with non-sensitive diagnostics.

Verification command (isolated mock worker, no live task changes):

```bash
cargo test -p videnoa-controller --test task20 submission_ownership::timed_out_submission_waits_for_restart_before_replay -- --exact
```

Expected: exactly one test passes, demonstrating the current restart-only recovery behavior.
