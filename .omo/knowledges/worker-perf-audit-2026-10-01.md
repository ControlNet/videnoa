# Worker runtime performance audit — 2026-10-01

Machine: NVIDIA A40-24Q (vGPU VM), Intel Icelake, 60 vCPU, dev at `8c2a60e`.
Workload: `bench_per_commit_fixture_1080p_120f.mkv` (1080p, 120 frames,
untagged colour), warmed sm86 TensorRT cache, SR=1/FI=3 (`anime-2x` then
`interpolation-2x`, 239 4K output frames) and SR-only. Harness, configs and logs
are in the ignored `.omo/benchmarks/worker-perf-audit-20261001/`.

This audit re-checked earlier notes independently. Several earlier conclusions
turned out to be artefacts of the dev machine, not of Videnoa.

## 1. Per-frame large-buffer churn is the dominant host cost (highest value)

- One SR->FI run (x265) had **20.1M minor page faults in the videnoa process**
  (about 82 GB first-touched, about 340 MB per output frame). The x265 ffmpeg
  child had 1.27M. The process spent **61 s system time vs 30 s wall**.
- Per-thread `/proc` sampling: the FI inference lanes used 27 s user and
  **21.5 s sys**. The tokio blocking stage threads used 13 s user and 24 s sys.
- `strace -f -c`: `munmap` was 1,732 calls averaging **8.3 ms each** (14.4 s).
- Micro-benchmark on this VM: a fresh 100 MB `to_vec` (alloc + fault + copy +
  free) took **87 ms**; a copy into a reused buffer took **10 ms**. FP16->FP32
  4K conversion took 16.8 ms into a reused buffer and 51.6 ms into a fresh one.
- Cause: every frame allocates 12-233 MB `Vec`/`Array` buffers (`to_owned`,
  crop `to_owned`, `to_vec`, `Array4::zeros`, `to_bits().collect()`). glibc
  serves these with mmap/munmap (its threshold cannot exceed 32 MB), so every
  frame pays page faults, kernel zeroing and TLB shootdowns. In a VM these are
  expensive.
- Validation with no code change: `LD_PRELOAD=libjemalloc.so.2
  MALLOC_CONF=dirty_decay_ms:-1,muzzy_decay_ms:-1`. The decoded framemd5 was
  identical.

  | SR=1/FI=3, FFmpeg 8.1, hevc_nvenc | glibc | jemalloc no-purge |
  |---|---:|---:|
  | wall (3 alternating pairs) | 28.2 / 30.0 / 26.5 s | 18.2 / 19.2 / 19.1 s |
  | FI inference | ~405-475 ms/pair | ~200-215 ms/pair |
  | SR inference | ~110 ms/frame | ~50 ms/frame |
  | FI postprocess | ~70-80 ms/frame | ~44 ms/frame |
  | sys time | 51-58 s | 20-23 s |
  | peak RSS | 7.4 GB | 8.0 GB |

  With libx265 the gain was about 13% (26.9/28.9 s -> 24.9/23.2 s), because
  x265 becomes the limiter. SR-only showed no gain: its direct-RGB path
  allocates little.
- Negative results: glibc `hugetlb=1` cut faults about 20x but wall time
  stayed in the noise. glibc `mmap_max=0` and mimalloc `MIMALLOC_PURGE_DELAY=-1`
  did not reduce faults. mimalloc and default jemalloc still return huge chunks
  to the OS. An allocator swap is therefore Linux-only: tikv-jemallocator does
  not support MSVC.
- The portable fix is in code:
  - FI inference: `output_view.to_owned()` -> crop `to_owned()` -> `to_vec()`
    makes three fresh 100 MB copies per output frame. Crop straight from the
    ORT view into one pooled buffer, or produce RGB inside the lane, which also
    parallelises the serial FI postprocess across lanes.
  - SR inference: `pad_f16_nchw` clones when no padding is needed, plus
    `Tensor::from_array(padded.clone())`, `to_owned()` and `to_bits().collect()`.
  - SR preprocess: `fill(ZERO)` and then a `to_bits().collect()` copy defeat
    the reused buffer.
  - FI preprocess: a fresh `Array4::zeros` per frame.
  - Recycle consumed frame buffers through a bounded pool.
  - If jemalloc is adopted on Linux, replace the job-end `malloc_trim(0)` in
    `VideoCompileContext::drop` with a jemalloc arena purge, to keep the idle
    memory contract.

## 2. Earlier encoder conclusions were measured with the wrong FFmpeg

- The dev tree has no `bin/`, so `runtime::command_for` falls back to system
  `/usr/bin/ffmpeg` **4.4.2**. Release packages bundle n8.1.x. All historical
  `.omo/benchmarks` runs likely used 4.4.
- 4K rgb24 through the VideoOutput filter chain: FFmpeg 4.4 costs about
  40 ms/frame of single-thread swscale; 8.1 about 5 ms. With 4.4, NVENC
  (97 ms/frame pipe blocking) was *slower* than x265.
- Benchmark with the bundled ffmpeg: symlink `bin/` in the working directory
  (cwd `bin/` is on the search path).

## 3. Colour bug found during the audit (correctness, also cheaper)

- The VideoOutput chain `format=<pf>,setparams=...bt709,zscale=range=limited`
  converts RGB->YUV with swscale's default **BT.601** matrix and then only
  *labels* the stream BT.709. Pure red gives Y=326 (BT.601), not about 250.
  After a tag-aware decode, (0,200,0) comes back as (0,167,0). This happens in
  FFmpeg 4.4 and 8.1 alike.
- zscale is a no-op copy here: swscale already produced the target format, so
  its dither never applies.
- The decoder `-pix_fmt rgb24` honours source tags, but untagged HD falls back
  to BT.601. BT.709 content decoded untagged gives (231,0,1)/(13,233,4).
  Consequences:
  - For **untagged** sources, the two BT.601 conversions cancel, so outputs
    currently look unchanged.
  - For **BT.709-tagged** sources, outputs shift.
- Fix both sides together, or untagged sources start shifting:
  - Decoder: scale with the source matrix; untagged sources with height >= 720
    are treated as BT.709.
  - Encoder: `scale=out_color_matrix=bt709:out_range=limited,format=<pf>,setparams=...:range=limited,setsar=1`,
    without zscale.
- The fixed encoder chain round-trips (253,0,0)/(0,200,0). With 8.1 + NVENC it
  costs 24 ms/frame vs 29; with 4.4 it costs 44 ms vs 54.

## 4. NVENC probe rejects valid GPUs

`EncoderConfig::run_nvenc_probe` encodes a 64x64 frame. On the A40 both
hevc_nvenc and h264_nvenc fail below 144x144 ("Frame dimensions are less than
the minimum supported value"), so every NVENC workflow on this GPU fails at
encoder creation. A 256x256 probe works; this was verified in a scratch build.

## 5. Smaller items

- There is no `[profile.release]` and no `target-cpu`, so builds target
  baseline x86-64. The FI RGB interleave loop goes 39 -> 30.5 ms per 4K frame
  with `x86-64-v3` (single thread); FP16 conversion is unchanged, since `half`
  detects F16C at runtime. It is a modest gain with portability cost, so prefer
  runtime-dispatched kernels over a global target-cpu.
- After fixing item 1, the next serial limits are FI postprocess and FI
  preprocess (about 44 and 70 ms/frame). The RGB interleave is store-bound;
  explicit SIMD shuffles are the remaining lever.
- SR `num_workers=2` with FFmpeg 8.1 raises SR stage throughput
  (5.8 -> 4.9 s per 120 frames). The encoder (about 30 ms/frame NVENC path) then
  limits SR-only.
- The first TensorRT session in a process takes about 4.2 s (CUDA/TRT init);
  later sessions take 0.03-0.26 s. This matters only for short clips.
- Each session creates a CPU intra-op pool: 244 threads, about 10 s user in a
  25 s run. Consider `with_intra_threads(1)` for GPU sessions.
- There is no TensorRT timing cache, so each new resolution rebuilds from
  scratch (8-12 min). A shared per-device timing cache would shorten builds for
  new shapes.
- `inference_output_memory_info` creates an `Allocator` and every call creates
  a new IoBinding per frame. Pageable 233 MB FI input goes H2D each call; a
  pinned concat buffer is an untested next experiment (GPU is ~30% busy).
- The decoder stage timer starts after `Iterator::next()` has produced the
  frame, so `avg_decode_ms` is always 0.0.

## Reproduce

```bash
S=.omo/benchmarks/worker-perf-audit-20261001
# needs: trt_cache symlink to warmed sm86 cache, bin -> FFmpeg 8.1 dir, BIN=<videnoa>
cd "$S" && ./run.sh combined-nvenc.json base-1
./run.sh combined-nvenc.json je-1 LD_PRELOAD=<jemalloc>/libjemalloc.so.2 \
  MALLOC_CONF=dirty_decay_ms:-1,muzzy_decay_ms:-1
# expected: je wall about 30% lower; sys time about 20 s vs about 55 s;
# identical framemd5
```
