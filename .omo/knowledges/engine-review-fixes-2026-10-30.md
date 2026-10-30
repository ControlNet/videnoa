# Engine fixes for design review findings E, G, H and I (2026-10-30)

Source: `design-review-2026-10-30.md`. These commits cover the engine side
of the findings (`crates/core` graph, compile, executor and nodes). Server
lifecycle work for E is done separately.

## E (engine half): cancellation is an outcome

- `node::WorkflowCancelled` is the error of a cancelled execution. Its
  message is still `workflow execution cancelled`.
- `node::is_cancellation(&anyhow::Error)` walks the error chain, so it
  also finds the error under `.context()`.
- Who returns it:
  - `ExecutionContext::check_cancelled()`;
  - `StreamingExecutor::execute_pipeline_stages`, both when cancelled
    before start and when the caller cancels mid-run. It used to return
    `Ok(())`.
  - Stage errors are still reported as themselves. Only the external
    cancel watch counts as a cancel.
- New `CompileContext::bind_execution_context` (default no-op). Compile
  calls it once. `VideoCompileContext` checks the job's cancellation
  before and after every SR/FI session load, and each inference lane
  loads its own session. A TensorRT build that is already running cannot
  be interrupted; it stops at the next check.
- For callers: the `Err` of
  `SequentialExecutor::execute_with_context_and_debug_hook` satisfies
  `is_cancellation(&err)` when the job was cancelled.
- Tests:
  - `streaming_executor::tests::test_cancel_signal_stops_pipeline`
  - `cancellation_before_start_reports_cancelled`
  - `stage_error_is_not_reported_as_cancellation`
  - `compile_context::tests::cancelled_compile_stops_before_loading_a_model`
  - `compile::tests::test_compile_binds_the_job_cancellation_to_the_compile_context`

## H: graphs that validate can run

- `compile_graph` runs in one topological pass, so a param node can consume
  a frame-chain node's scalar output, for example
  `VideoInput.source_path → PathDivider → PathJoiner →
  VideoOutput.output_path`.
- `validate()` loads every `Workflow` node's target
  (`workflow_io::validate_nested_workflow`) and rejects one that:
  - is empty or unreadable;
  - nests itself, or is nested deeper than 10 levels;
  - has VideoFrames edges. The nested executor is scalar-only.

  It also checks the workflows a target nests in turn.
- Tests:
  - `tests/workflow_validation.rs` (needs `ffmpeg` on PATH, else skips):
    - `scalar_node_downstream_of_frame_chain_node_runs`
    - `nested_video_workflow_is_rejected_at_submission`
  - `compile::tests::test_compile_runs_scalar_nodes_that_consume_frame_chain_outputs`

## G: workflow format

- **schema_version.**
  - The `PipelineGraph` serde writes `schema_version`.
    `WORKFLOW_SCHEMA_VERSION` is 1, and a document without it is legacy
    version 1.
  - `graph::migrate_workflow` is the single place for migrations (identity
    today).
  - A newer or invalid version is rejected.
  - `POST /api/workflows` stamps the version through
    `graph::versioned_workflow`. This is a one-line call in
    `server/mod.rs`.
- **Unknown params.**
  - `validate()` rejects a param that is neither an input port nor accepted
    by `Node::accepts_config_param`, with
    `node '<id>' of type '<type>' has unknown param '<key>'`.
  - Allowed config keys, from an inventory of presets, web, server and
    controller:

    | Node | Allowed config keys |
    |---|---|
    | WorkflowInput | any key (the injected port values) |
    | WorkflowOutput | `ports` |
    | Workflow | `workflow_path`, `interface_inputs`, `interface_outputs` |
    | StringTemplate | `strN` (stale after `num_input` is lowered) |

  - `inference_concurrency` still gets its "renamed to num_workers" error.
  - Tests:
    - `graph::tests::test_validate_rejects_unknown_param_key`
    - `test_validate_accepts_every_param_key_the_writers_use`
    - `test_shipped_presets_validate`
- **device_id.**
  - SR and FI now have a `device_id` port, passed to the TensorRT EP and
    the CUDA EP and used in the TensorRT cache key.
  - CUDA-pinned host `MemoryInfo` stays on device 0. ORT keeps pinned
    memory on the CPU device, id 0.
  - `tests/descriptor_ports.rs` compares descriptor and runtime ports for
    every registered node. The only allow-listed difference is
    `Workflow.workflow_path`.

## I: TensorRT cache

- **Key format.**
  `trt-{tensorrt}_ort-{onnxruntime}_device-{id}_{sha256}_{h}x{w}[_tile-…]`.
  - `tensorrt` is `getInferLibVersion()` from `libnvinfer.so.10`
    (`nvinfer_10.dll` on Windows), e.g. `100900`. It is `none` if the
    library cannot be loaded.
  - `onnxruntime` is the `git-commit-id` field of `ort::info()`. The bundled
    1.23.2 build reports `git-branch=HEAD` and no version. If there is no
    commit id, the first 12 hex digits of the SHA-256 of the build info are
    used.
  - `VideoCompileContext` detects these lazily, only for the TensorRT
    backend. Tests inject them, because CI has no ORT.
- **Atomic placement** (`backend::open_trt_cache_entry`).
  - The engine is built in `<key>.tmp-<pid>-<nanos>` and renamed into place
    under `File::lock` on `<key>.lock`. A waiting process loads the result.
  - Abandoned tmp dirs are removed under the lock.
  - Assumption: the TensorRT EP writes the engine during session creation.
- **Broken entries.** If loading fails with "could not deserialize
  engine", the entry is renamed to `<key>.broken-<ms>` and built again,
  once. The entry is re-checked under the lock first.
- **Upgrade cost.** Every existing engine is rebuilt once after the
  upgrade. Old directories stay on disk until deleted by hand.
- Tests: `backend::tests::*trt_cache*`, plus
  `concurrent_opens_build_a_trt_cache_entry_once`. No GPU is used.
