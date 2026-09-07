# Videnoa idle host memory diagnosis

Follow-up: [allocator experiments and verified fix](videnoa-memory-reclamation-2026-09-06.md)
confirmed free-heap retention, added job-boundary reclamation and an eight-arena
startup default, and fixed cancellation resource lifetimes. The historical
read-only observations below predate those experiments.

## Scope and evidence

Read-only runtime diagnosis on 2026-09-06, approximately 18:29-18:33 AEST.
No worker restart, task submission/cancellation, debugger attachment, allocator
mutation, or production code change was performed. Source inspected at 4097855;
the running binary was built earlier that day, so source findings are not a
binary-level verification.

- Worker PID 1791280 runs `target/release/videnoa` from this repository.
- RSS: approximately 25.75 GiB; anonymous RSS: approximately 23.87 GiB.
  Peak RSS was approximately 28.63 GiB. Worker swap usage was zero.
- The job API returned 27 records: 16 completed, 10 cancelled, one failed,
  and zero queued/running. Latest completion: 18:17:49 AEST. Idle RSS remained
  near 25.75 GiB across the observation window.
- The job response was only 16,232 bytes. Job metadata itself does not contain
  inference sessions or frame buffers.
- GPU memory for this worker was 297 MiB. No active inference worker threads
  remained: 67 threads included 60 Tokio runtime workers, four RGB workers,
  two CUDA threads, and the main thread. Open file descriptor count was 45.
- Historical `/api/performance/export` samples at 16:00-16:01 AEST recorded
  approximately 7.522 GiB process RSS. New observations at 18:32 recorded
  approximately 25.745 GiB: an increase of 18.223 GiB. There are no intermediate
  samples establishing the shape of that increase or attribution per task.
  Eleven jobs started and completed between those observation groups.
- Logs between 17:00 and 18:33 AEST contained 16 TensorRT session initializations,
  16 existing-cache session-ready messages, and 11 job completions; no engine
  cache update or session initialization failure messages in that interval.
- The host exposes 60 CPUs and runs glibc 2.35. Allocator arena, trim, mmap,
  and Tokio worker-count overrides were unset in the worker environment.
- 390 anonymous writable mappings of at least 1 MiB started on 64 MiB
  boundaries; together they held 23.696 GiB RSS. Most were approximately
  64 MiB. These are arena-like mappings, not a measured count of malloc arenas.
- A TensorRT builder resource library mapping accounts for approximately
  1.46 GiB of file-backed RSS. Runtime maps identify the builder resource as
  version 10.9.0; do not assume the older environment notes describe the loaded
  runtime exactly.

## Interpretation and limits

The strongest hypothesis is glibc heap retention/fragmentation from repeated
multithreaded inference workloads. This is process memory, not system page-cache
accounting or currently queued frames. The mapping layout strongly supports an
allocator explanation, but does not prove that the resident chunks are free.
Native-library leaks or retained live allocations remain possible.

Opening `/proc/1791280/mem` for read-only heap inspection returned permission
denied. No attempt was made to change ptrace settings or bypass that restriction.
Without allocator statistics or allocation profiling, do not claim that all
23.7 GiB is reclaimable, or that a specific malloc setting fixes the issue.

The checked streaming executor uses bounded frame channels, awaits stage handles,
and aborts/awaits its external cancellation watcher. Parallel processor and
interpolator implementations join their worker threads. Session ownership is
scoped to per-job pipeline stages; no global session cache was found in the
inspected path. Low idle GPU usage supports, but does not prove, session teardown.

## Separate source defect

`crates/core/src/server/mod.rs`, in `run_job` around line 2546, spawns
`_cancel_bridge` waiting only for `CancellationToken::cancelled()`. The handle is
dropped without aborting or joining on normal completion or execution failure.
Dropping a Tokio JoinHandle detaches its task. Successful jobs therefore leave
pending cancellation bridge tasks until cancellation or runtime shutdown.

The closure holds a token and a watch sender, not the compiled graph, frames, or
inference sessions. This is a real small lifecycle leak in the inspected source,
but cannot plausibly explain 26 GiB for this job count. A future fix should ensure
bridge teardown on success, failure, and unwinding and test completion while the
original job cancellation token remains uncancelled.

## Follow-up verification

Preserve the current process for evidence. In an isolated reproduction, collect
`mallinfo2`/`malloc_info` after completed jobs and compare allocated versus free
heap bytes. A controlled `malloc_trim(0)` experiment in that isolated process
can determine how much retained RSS can be returned. A small response does not
rule out fragmentation because live allocations can pin pages.

Compare identical repeated jobs with the default allocator and a separately
started process using `MALLOC_ARENA_MAX=4`. Preserve the same models, backend,
warm caches, input sequence, and thread configuration. Record RSS after each
completion plus throughput and output correctness. The expected supporting
signal is a lower, stable idle plateau without material throughput regression.
If live allocation bytes keep increasing, profile allocation stacks rather
than treating allocator tuning as a fix. The setting affects new processes
only and is not a total memory limit.

Read-only commands for the observed process (PID is session-specific):

```bash
ps -p 1791280 -o pid,comm,rss,vsz,nlwp,etime
cat /proc/1791280/smaps_rollup
rg '^(VmHWM|VmRSS|RssAnon|VmSwap|Threads):' /proc/1791280/status
nvidia-smi --query-compute-apps=pid,process_name,used_memory --format=csv
```

References:

- [glibc 2.35 allocator](https://www.gnu.org/software/libc/manual/2.35/html_node/The-GNU-Allocator.html)
- [glibc 2.35 manual, arena defaults and tuning](https://www.gnu.org/software/libc/manual/2.35/pdf/libc.pdf)
- [glibc allocator statistics](https://sourceware.org/glibc/manual/latest/html_node/Statistics-of-Malloc.html)
- [Tokio JoinHandle detachment](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html)
