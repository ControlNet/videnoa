# Worker memory reclamation

## Confirmed cause

The high idle RSS observed in the earlier diagnosis was predominantly glibc heap
retention and fragmentation, not live video frames, active GPU sessions, or the
Controller task list. Isolated repetitions of the user's actual TensorRT
workflows reproduced several GiB of idle growth per job. A diagnostic preload
library sampled `mallinfo2`; live allocations stayed near 321 MiB while free
allocator space grew with RSS.

With the original binary and six 120-frame RIFE jobs, RSS reached 16.865 GiB.
`mallinfo2` reported 16.111 GiB of free heap space and 321.96 MiB of live heap
allocations. A diagnostic `malloc_trim(0)` reduced RSS to 1.042 GiB without
restarting the worker or releasing any live allocation. Four super-resolution
jobs similarly reached 10.595 GiB; trimming reduced RSS to 0.847 GiB.

The benchmark input is an actual excerpt of the repository's `test-30s.mkv`,
1280x720 at 30 fps. Models are the real AnimeJaNai V3 L1 FP16 x2 and RIFE v4.26
ONNX files. TensorRT engine caches were warm and reused. There are no synthetic
models or substituted production implementations. Only allocator unit fixtures
and the intentionally invalid encoder failure case use synthetic test inputs.

## Why both allocator changes are needed

Trimming at the video-job boundary removes the large per-job RSS increase, but
alone does not prevent slow fragmentation across glibc arenas: a 60-job test
with eight-frame excerpts still ended at approximately 1.80 GiB idle RSS.
Live allocation bytes remained essentially stable, around 323 MiB.

Limiting arenas to eight in a separate experiment reduced that residual growth:
24 jobs ended near 0.96 GiB. `malloc_info` independently confirmed eight arenas.
This limit controls allocator heaps, not Tokio or inference worker counts and
not total memory. It cannot replace trimming: several GiB of free virtual heap
space can remain even with a small arena count. Virtual address reservation
must not be reported as resident RAM.

## Implementation

- `runtime::configure_host_memory` sets `M_ARENA_MAX=8` on Linux/glibc, once,
  unless `MALLOC_ARENA_MAX` or `glibc.malloc.arena_max` in `GLIBC_TUNABLES` is
  explicitly provided. It is an unsafe startup-only API because malloc policy
  must be configured before concurrent allocations. CLI/server `main` calls it
  before creating Tokio; desktop `main` calls it before Tauri/native setup.
- `VideoCompileContext::drop` first clears any stages retained after partial
  compilation, then calls `malloc_trim(0)` on Linux/glibc. Normal streaming
  execution joins its stages before this per-job context is dropped. Errors
  and unwinding also reclaim unused pages. No trimming happens per frame.
- The server cancellation bridge now exits when either cancellation arrives
  or the executor drops its watch receiver. Previously every successful job
  left a detached task waiting indefinitely for its retained token.
- The executor's default uncancelled watch no longer deliberately leaks its
  sender with `mem::forget`. A closed false watch is already supported by the
  streaming cancellation watcher and does not truncate the pipeline.

Allocator policy/reclamation are no-ops on non-glibc platforms. The cancellation
lifecycle fixes apply to all platforms. Explicit allocator overrides are
preserved, including opting back into glibc's own default via an explicit zero.

## Regression tests and validation method

- A subprocess test retains live sentinels between 96 MiB of stage allocations,
  disables automatic malloc trimming/mmap, and verifies context destruction
  returns more than 64 MiB of RSS on success, error, and unwind. It failed before
  the fix (approximately 106 MiB before teardown and 108 MiB afterward) and passes
  with explicit reclamation. Live sentinel contents must remain intact.
- `tests/allocator_policy.rs` runs without libtest so its startup policy executes
  before any test-harness threads exist. Thirty-two allocation workers verify
  the eight-arena default and explicit environment/tunable overrides of two and
  three arenas. The original default allowed 34 arenas in the failing test.
- Cancellation bridge tests verify both automatic completion without cancelling
  the original token and delivery of actual cancellation. A closed-false-watch
  streaming test verifies all ten fixture frames reach the sink.
- Before the arena limit was added, real workloads already verified 20 FI jobs,
  12 SR jobs, nine alternating SR/FI/combined jobs, four intentional encoder
  failures, and cancellation followed by two successful full 30-second input
  jobs. Selected first/last and mixed-workflow SR/FI outputs had identical decoded
  frame hashes to the original binary. CLI execution without an external
  cancellation sender also completed with all 15 expected output frames from
  eight input frames.

Original and fixed SR output: 2560x1440, 30 fps, 120 frames. Original and fixed FI
output: 1280x720, 60 fps, 239 frames. Exact decoded-frame MD5 stream SHA-256:

- FI: `a2eb0a3ffa33c72e7f6d768f4a842f0ea8536d2ec7c29278ee2e76570fc57f8a`
- SR: `6d23b34bf53b132c3358ce6c6b6d506c38361c156aba70f68d9b637a584b282d`

The diagnostic files and private runtime logs are under
`.omo/benchmarks/memory-20260906/`. Final-binary tests use no preload probe and
no allocator environment override, exercising the actual startup defaults.
Some stress experiments run concurrently on separate ports; their wall times
are not controlled performance comparisons.

## Final binary results

All final runs use the built-in policy, without preload instrumentation or an
arena environment override. The complete sanitized sample series is in
`../benchmarks/memory-20260906/summary.json` relative to `.omo/knowledges`.

| Workload | Outcome | Idle RSS |
| --- | --- | --- |
| Same six 120-frame FI jobs as the original reproduction | 6 completed; last output matches original decoded hash | 970.38 MiB (original: 17,269.80 MiB before diagnostic trimming) |
| 60 eight-frame FI jobs, one persistent worker | 60 completed | 999.38 MiB after 15 seconds idle |
| Alternating SR, FI, and combined pipelines on 120 frames | 6 completed; SR/FI hashes match original; combined repeats match each other | 900.79 MiB |
| Intentionally invalid encoder | 2 failed as expected | 909.70 MiB |
| Cancel running FI, then process a full 30-second input | Cancellation acknowledged; successor completed | 923.57 MiB |

In the 60-job final run, job 30 ended at 995.56 MiB and job 60 at 999.36 MiB.
The last 30 completions ranged from 971.95 to 999.36 MiB; the earlier multi-GiB
per-job rise and the trim-only fragmentation trend did not recur. The matched
six-job comparison reduced idle RSS by approximately 94.4%. Required memory
during inference still depends on the model, dimensions, and worker count.

Final quality results: 613 core unit tests, 22 app unit tests, three crash-hook
tests, and all three standalone allocator-policy cases passed. Ten pre-existing
core tests remain ignored. Strict core Clippy across all targets, formatting,
the release worker build, and the Linux desktop compile check passed. Every
worker started by this investigation was stopped; original job data was not
used as a test worker data directory.

Quality commands:

```bash
cargo test -p videnoa-core -p videnoa-app --lib --tests
cargo clippy -p videnoa-core --all-targets -- -D warnings
cargo fmt --all -- --check
export ORT_DYLIB_PATH="$PWD/lib/libonnxruntime.so"
export TRT_LIBS="$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs"
export LD_LIBRARY_PATH="$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:${LD_LIBRARY_PATH:-}"
export PKG_CONFIG_PATH="$HOME/miniconda3/envs/anime/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
cargo build --release --locked -p videnoa-app
cargo check -p videnoa-desktop --locked
```

Expected signals: tests pass, allocator-policy subprocess cases pass, Clippy and
format checks exit zero, and the worker/desktop compile successfully. The broad
app-library Clippy probe found pre-existing callback type-complexity, nested-if,
and approximate-constant test diagnostics in unchanged `crates/app/src/lib.rs`;
those are outside this memory fix. The strict core gate is checked separately.

References:

- [glibc allocator arenas](https://www.gnu.org/software/libc/manual/2.35/html_node/The-GNU-Allocator.html)
- [malloc tuning and arena limits](https://man7.org/linux/man-pages/man3/mallopt.3.html)
- [malloc_trim reclaims free pages across arenas](https://man7.org/linux/man-pages/man3/malloc_trim.3.html)
- [glibc malloc statistics](https://sourceware.org/glibc/manual/latest/html_node/Statistics-of-Malloc.html)
