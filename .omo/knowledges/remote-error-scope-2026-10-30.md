# Remote error scope in the Controller (2026-10-30)

Fixes for design-review findings A, B, L, the controller side of C, and
the "transport version skew looks like an outage" Low item
(`design-review-2026-10-30.md`).

## Rule: who is the error about?

- `VidenoaClientError::is_worker_level()` (remote/error.rs): transient
  (network, timeout, stall, 5xx), `is_authentication()` (401/403), 429,
  `UnexpectedStatus` (1xx/3xx) and `Incompatible`. No single request is to
  blame, so these never fail a task and are never process-fatal. The task is
  deferred and the worker backed off (`Reconciler::defer_for_worker`).
- 401/403 mark the worker offline with `next_health_check_at = None`,
  `health_retry_count >= 1`, `last_error = AUTHENTICATION_FAILED` (shared
  with the health service). The Orchestrator gate then holds the worker's
  tasks until a probe sees working credentials; the state is durable, so a
  restart does not loop on the rejection.
- Only job-specific evidence fails a task: a job-id 404 after the health
  check passed, a job whose identity contradicts the attempt, or a terminal
  job status reported by the worker.
- An unreadable poll reply (`MalformedPayload`, e.g. a status a newer worker
  adds) means "still running": warn and keep polling. Do not add
  `#[serde(other)]` for this; it is classified at the error-handling layer.
- Submission: 400/404/other 4xx are definite rejections
  (`RemoteSubmissionFailed`); 409, malformed and oversized replies stay
  `RemoteStateAmbiguous`. A cancel-during-submit 401/403 is not proof that
  the job was never accepted.
- Remote cleanup retries worker-level errors; only a definite rejection is
  `CleanupFailed`.
- Identity contradiction: the task fails, then a Queued/Running job at the
  task's job ID gets a best-effort `DELETE /api/jobs/{id}`
  (`cancel_abandoned_job`). Safe because worker job IDs are random UUIDv4 per
  submission.
- Controller-local errors (LocalIo, InvalidFilePath, EndpointUrl) and
  lifecycle persistence errors stay fatal, as before. The one exception is
  the reconcile health gate: any `health()` error there defers the task.

## Tunnel incompatibility

- CONNECT answered with 400, an ALPN mismatch (TLS alert 120,
  `TransportErrorCode::crypto(120)`) or a QUIC version mismatch is
  `TunnelError::Protocol` -> `VidenoaClientError::Incompatible`, non-transient.
  Health shows "worker tunnel is incompatible with this Controller; update the
  worker or the Controller" instead of an ordinary outage.

## Catalog

- `/api/workflows` and `/api/presets` are read entry by entry. An unreadable
  entry with a name, or an unreadable/oversized interface reply, goes to
  `invalid_workflows` with a reason; the worker stays online. A listing that
  is not a JSON array still fails the refresh.
- A preset whose run name equals a saved workflow's run name is listed
  invalid: the worker's `/api/run` resolves saved workflows first, so the
  preset is unreachable. The workflow stays eligible.

## Tests

- task20 `remote_scope.rs`: `health_replies_that_are_not_ok_defer_reconciliation`,
  `health_rejecting_credentials_blocks_the_worker`,
  `rejected_remote_cancel_defers_instead_of_stopping_the_controller`,
  `rejected_cancel_after_confirming_submission_defers`,
  `health_payload_drift_during_processing_does_not_stop_orchestration`,
  `health_not_found_during_processing_does_not_stop_orchestration`,
  `poll_replies_about_the_worker_defer_instead_of_failing_the_task`,
  `unreadable_job_status_keeps_polling`,
  `rotated_worker_password_during_processing_waits_for_the_new_password`
  (passes before the fix too, because the health service usually wins the
  race), `rate_limited_poll_during_processing_does_not_fail_the_task`,
  `rejected_credentials_on_submission_defer_instead_of_failing`,
  `rejected_credentials_while_cancelling_a_submission_keep_its_job`,
  `remote_cleanup_retries_rejected_credentials_and_rate_limits`.
- task20 `remote_isolation.rs`: `malformed_poll_keeps_polling_while_other_work_completes`
  (inverted from the old malformed-poll-fails test).
- mock_videnoa `recovery_contracts.rs`:
  `contradictory_remote_identity_is_nonretryable_ambiguity` now also asserts
  one `JobCancel` and no active job.
- `videnoa_client.rs`: `a_malformed_preset_is_listed_invalid_without_hiding_the_others`,
  `a_malformed_workflow_interface_is_listed_invalid_without_hiding_the_others`,
  `a_preset_shadowed_by_a_saved_workflow_is_invalid`.
- transport `tunnel.rs` `incompatibility_tests`; controller
  `remote::iroh::classification_tests::tunnel_protocol_failures_are_incompatibility_not_outage`,
  probe `incompatibility_has_its_own_health_message`.

## Harness notes

- Mock `JobCancel` and `Interface` routes now honour one-shot
  `Fault::Response`; `DeleteOutcome` has `Unauthorized` and `RateLimited`.
- e2e "Controller keeps running" checks use a fixed 5 s window, not
  `wait_for_orchestration_error` (scaled 30 s timeout).
- The `sse_shutdown_process` test needs `controller-web/dist`; a fresh
  worktree lacks it (copy it from the main checkout or build the web UI).
