# Wave-1 leftovers: after-stream nodes, desktop server settings, workflow picker (2026-10-30)

## Nodes that consume the sink's outputs run after the stream

- Problem: the H fix runs every scalar node during compilation, so a node fed
  by `VideoOutput.output_path` (a "done" `HttpRequest`, a `Print`, a
  `WorkflowOutput`) ran before any frame was encoded.
- `compile_graph` now skips every node reachable from the frame-chain sink
  (any connection, transitively) and returns it in
  `CompiledPipeline::after_stream` (`AfterStreamNodes`, with the outputs of
  the nodes compile ran). The sink itself still runs at compile time; its
  `execute()` already knows the final `output_path`.
- `SequentialExecutor` runs `after_stream.run(...)` only after
  `execute_pipeline_stages` returned `Ok` (the output is published by
  `finish_unless_cancelled` inside it). A failed or cancelled stream returns
  early, so these nodes never run; `run` also checks cancellation before each
  node. A node's error fails the run (the job becomes `failed`).
- Scalar consumers of source-side outputs (`VideoInput.source_path ->
  PathDivider -> PathJoiner -> VideoOutput.output_path`) still run before the
  stream. A node downstream of the sink can never feed a frame-chain node (that
  would be a cycle), so the split is always valid.
- The rule is "downstream of the sink", so consumers of `StreamOutput.output_url`
  also run after the stream now.
- Callers of the public `compile_graph` that run the pipeline themselves must
  call `after_stream.run` too. Today only the executor does.
- Tests: `executor::tests::nodes_that_consume_the_sink_output_run_after_it_is_published`,
  `..._do_not_run_when_the_stream_fails`, `..._do_not_run_when_the_stream_is_cancelled`,
  `a_failing_consumer_of_the_sink_output_fails_the_run`;
  `tests/workflow_validation.rs::webhook_on_the_output_path_fires_after_the_output_is_published`
  (needs ffmpeg, else skips; the webhook endpoint checks the posted path is a file).

## Desktop Settings: server host/port

- The desktop binds 127.0.0.1 on a random port and ignores `server.host` and
  `server.port`. With `isDesktopRuntime()` (`web/src/lib/runtime-desktop.ts`,
  true when `__TAURI__.window` exists) the Server section shows "Not
  applicable" and a hint instead of the values. The saved config keeps them.
- Tests: `SettingsPage.test.tsx` "SettingsPage server section".

## WorkflowPathPicker

- Loads presets and workflows with `allSettled`; a failed list shows "Could
  not load the workflow list" with Retry under the select (role `alert`), and
  the "No workflows with interfaces" row is shown only when both lists loaded.
- Tests: `WorkflowPathPicker.test.tsx` "says the list failed to load and
  retries it", "an empty list that loaded is not an error".
