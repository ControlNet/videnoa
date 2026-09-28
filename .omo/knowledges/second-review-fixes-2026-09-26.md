# Second review fixes, 2026-09-26

Scope: all seven findings in `project-second-review-2026-09-26.md`, starting
from `1d23540`. The earlier deferred Downloader basename collision is outside
this change.

## Decoder and job deletion

`VideoDecoder` now verifies FFmpeg's exit status at EOF and returns an error for
partial raw frames. The iterator emits an error once, then ends. Streaming
execution already propagates decoder errors to the failed job state. Unit tests
inject complete/partial raw output and exit failures. The separate
`decoder_failure` integration executable decodes one real FFmpeg frame, injects
exit code 7, and asserts the job fails. It is isolated because executable
resolution for this fixture uses a temporary current working directory.

History deletion holds the runtime entry while deleting its durable row. A
database failure leaves both cancellation and progress delivery untouched.
Only successful deletion removes and cancels an active job. Executor snapshots
use UPDATE rather than UPSERT, so a late transition cannot recreate a deleted
row. A SQLite trigger injects deletion failure while a real queued executor is
waiting for admission; the test checks that it can subsequently complete.

## Preview access and lifecycle

Preview files live in `data_dir/preview-cache/session-*`. The cache holds an
exclusive filesystem lock, and active sessions retain that lock through shared
ownership. Startup removes orphan session directories only within this owned,
locked cache. It never sweeps legacy `/tmp/videnoa-preview-*` directories because
those have no ownership record distinguishing other Worker instances.

Session directories are RAII-owned. Failed extraction, release, expiry, and
application shutdown clean up their files. A session lease keeps files alive
through active reads and processing. `DELETE /api/preview/{preview_id}` removes
the session from discovery immediately; physical cleanup follows the last
active lease. The frontend releases sessions on close, unmount, changing the
video path, and re-extraction, including extraction responses that arrive late.
Failed release requests are covered by server expiry.

Policy constants in `preview_cache.rs`:

- 8 sessions, including pending extractions and released-but-active leases.
- 2 simultaneous extractions; excess requests return HTTP 429.
- 120 seconds total for probing plus extraction.
- 30 minutes of idle lifetime, with a 60-second sweep interval and lazy expiry.
- 256 MiB retained per session; 2 GiB maximum across eight retained sessions.
- 8 processed results per session, counting pending processing reservations.
  A new extraction resets this result allowance. Failed processing returns its
  reservation.

FFmpeg/ffprobe use Tokio subprocesses with bounded captured output (64 KiB per
stream, continuously drained), timeout termination/reaping, and kill-on-drop
for cancellation. Extraction checks directory size while the process runs and
at completion. Processing checks size after output and removes an over-budget
result. The disk budget is a retained-data limit, not an OS filesystem quota:
in-flight writes can temporarily exceed it before the next check.

The frame route accepts only generated `frame_0001.png` through `frame_0100.png`
and canonical `processed-<uuid>.png` names. Capability-relative file opening
confines symlink resolution to the session. File reads run in blocking tasks
with session leases; no session-map guard crosses an await.

Tests cover encoded absolute/parent paths, symlink escapes, expiry, lease-safe
release, restart cleanup, cache ownership, byte/result/session limits, HTTP
admission, failed extraction cleanup, subprocess timeout, and cancellation. The
Linux cancellation test checks that the child PID disappears and the temporary
session is removed.

## Frontend request ownership

Worker submissions, named runs, and reruns immediately retain and return the
server's creation receipt. Refresh runs separately and reports its own error.
Successful history deletion also remains successful if refreshing fails. The
Jobs page displays the refresh warning and retries through normal polling.
List generations prevent late responses from undoing a newer response or an
accepted mutation; an in-flight newer poll does not suppress an older completed
poll indefinitely.

Controller global counts subscribe independently of the visible task filter.
Updates coalesce into a trailing refresh with at most one counts request in
flight. Events received during a slow read schedule a subsequent refresh rather
than cancelling it repeatedly. Page refreshes do not reset this counts reader;
manual retry and global invalidation still refresh both. Cleanup aborts requests
and removes the event subscription and timer.

## Verification commands

Use the recorded conda `anime` runtime libraries:

```bash
export ORT_DYLIB_PATH=$PWD/lib/libonnxruntime.so
export TRT_LIBS=$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs
export LD_LIBRARY_PATH=$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:$LD_LIBRARY_PATH
export PKG_CONFIG_PATH=$HOME/miniconda3/envs/anime/lib/pkgconfig:$PKG_CONFIG_PATH

cargo fmt --all --check
cargo test --locked -p videnoa-core -p videnoa-app -p videnoa-transport -p videnoa-desktop --all-targets -- --test-threads=4
cargo test --locked -p videnoa-controller --all-targets -- --test-threads=1
cargo test --locked -p videnoa-core --lib --test decoder_failure --test server_regressions -- --test-threads=4
cargo test --locked -p videnoa-core --test server_regressions preview_superresolution_cuda_produces_upscaled_png -- --ignored --nocapture
cargo clippy --locked -p videnoa-core -p videnoa-app --all-targets -- -D warnings
npm --prefix web test
npm --prefix web run lint
npm --prefix web run build
npm --prefix controller-web test
npm --prefix controller-web run lint
npm --prefix controller-web run build
```

All commands should exit zero. The real CUDA test loads the actual RealESRGAN
model and checks a 32x32 input becomes a 128x128 PNG while preserving the source.
Fault injection, generated media, sparse quota files, and mocked HTTP responses
are explicitly test fixtures; no production model or implementation is replaced.

Validation is on Linux. Windows runtime, native browser end-to-end behavior,
TensorRT engine compilation, and release packaging are not covered by these
checks. Vite reports existing bundle-size and third-party annotation warnings.

## Recorded results

- Worker/Core/App/Transport/Desktop all targets: 676 passed, 12 ignored.
- Controller all-target coverage: 600 tests passed and 1 ignored, across the
  initial run and the completion/isolated reruns detailed below.
- Final Core library plus decoder and server API regressions: 647 passed,
  11 ignored (included in the preceding result, not additional unique tests).
- Real CUDA preview: 1 passed separately from the default ignored tests.
- Worker frontend: 208 passed; Controller frontend: 184 passed.
- Both frontend builds and linters, Rust formatting, and strict Core/App Clippy
  passed. Staged secret-guard scan found no secrets.

The first workspace run with four test threads stopped at three
`task21_concurrency` database-fixture initialization failures (`PoolTimedOut`).
Its complete 58-test rerun with one thread passed. During completion of the
remaining targets, `task21_security` passed 53 tests and had one database pool
timeout in `task14::retry_faults::processing_retry_rejects_missing_remote_job`.
That test passed when rerun alone after other compilation/testing finished.
These transient failures are recorded rather than presenting the initial full
workspace command as a clean run. No Controller Rust source was changed.

Logs for this session are in `/tmp/videnoa-fixes-*.log`; the tracked tests and
commands above are the durable reproduction path.
