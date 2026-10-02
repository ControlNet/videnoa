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

## Implementation results (branch `perf/worker-runtime`)

Same machine and fixture. The vGPU makes wall time noisy (the same build
varies 21-30 s), so compare alternating A/B pairs after a warm-up run and use
stage averages and sys time, not single wall times.

### Dev media tools and colour fix (audit items 2 and 3)

- `79ddfa9`: `scripts/setup_dev_media_tools.sh` installs the release FFmpeg
  bundle into `<repo>/bin`, so dev runs and benchmarks use FFmpeg 8.1, not the
  system 4.4.
- `a300113`: the decoder scales with the source matrix (tag, else BT.709 for
  sizes of at least 1280 wide or more than 576 high), and the encoder, stream
  output and preview convert with `out_color_matrix=bt709:out_range=limited`.
  After v0.1.7 the untagged fallback became BT.709 at every size, by request:
  the mpv-style SD -> BT.601 guess was dropped. Tagged SD sources (for example
  `smpte170m`) still decode with their tag.
  Ignored round-trip tests:
  - `decoder_recovers_source_rgb_for_tagged_and_untagged_matrices`
  - `encoded_output_decodes_back_to_the_source_colors`

  Run them with `PATH=$PWD/bin:$PATH`.
- The NVENC 64x64 probe (item 4) is parked by request. The benchmark binaries
  carry a local-only 256x256 patch.

### Frame buffer pool (`b221df5`, audit item 1)

- `crates/core/src/frame_pool.rs`: one `FramePool` per job, owned by
  `VideoCompileContext`. The decoder, SR/FI micro-stages and encoder take
  buffers from it and return them. The encoder gets frames back through the
  new `FrameSink::release_frame` hook.
- A free buffer is reused when its capacity is within 25% above the request,
  so padded and cropped 4K FP32 frames share one class. Taken buffers keep old
  contents, so every writer must overwrite all elements; the tests seed NaN
  buffers to check this. The pool keeps only buffers whose size some stage has
  requested, so a sink behind a non-pooled stage cannot pin dead buffers. The
  cap is 32 per element type.
- The copies were removed at their source:
  - FI inference crops from the ORT output view once, instead of three copies.
  - FI postprocess converts from the padded planes.
  - SR inference feeds aligned frames to ORT without a copy, and pads or crops
    into pooled buffers.
  - SR preprocess writes FP16 bits in place.
- SR=1/FI=3, NVENC, 3 alternating pairs:

  | | before | pool |
  |---|---:|---:|
  | wall | 32.5 / 25.3 / 31.0 s | 21.8 / 24.4 / 22.9 s |
  | sys | ~48 s | 11-13 s |
  | minor faults (incl. children) | 19.5M | 2.5M |
  | SR inference | ~125 ms | 53-66 ms |
  | FI postprocess | ~65 ms | ~22 ms |

  SR-only went 14.6 -> 11.9 s and x265 SR->FI 31.3 -> 24.1 s. Decoded
  framemd5 is identical for NVENC, x265 and SR-only.
- The pool hit rate is about 95%; misses are the pipeline filling up. The job
  end logs a `Frame pool summary` with reuse and allocation counts.
- Videnoa's steady state now has about 0 page faults per second. All 1.9M
  remaining faults happen in the first ~11 s: CUDA/TRT init, engine
  deserialisation and pool warm-up.
- jemalloc on top of the pool no longer changes wall time. The remaining
  steady-state heap churn (128 MB arena mmaps, 64 MB munmaps) is in the
  **ffmpeg encoder child** (`dmx0:rawvideo`/`vf#0:0` threads allocate a fresh
  packet per frame). glibc heap-retention tunables on the child cut faults
  2.54M -> 2.03M with no wall change, so this was dropped.

### Pinned RIFE input (`5364c12`)

- Each FI lane keeps one `[1,7,H,W]` input tensor, CUDA-pinned when the
  session supports it, and fills it in place. This replaces a 233 MB pageable
  array per pair.
- ORT `Allocator`/`MemoryInfo` are not `Send`/`Sync`. A stage that stores them
  needs a `Mutex` wrapper; it is accessed through `get_mut`, so there is no
  locking cost.
- 1 FI lane, 3 alternating pairs: FI inference 145-171 -> 109-149 ms/pair,
  wall 27.2/30.8/23.2 -> 25.8/23.4/19.2 s. RSS is +150-370 MB, since ORT's
  pinned arena rounds up.
- Test pitfall: `FrameInterpolationNode::process_frame_pair` caches img1 as the
  next img0, so a reference comparison must call `disable_pair_cache()`. The
  GPU test `micro_stages_match_the_single_node_path` does this.

### The pipeline is now GPU-bound; FI lanes do not scale here

- FI `session.run` time grows linearly with lane count, so throughput is
  constant at about 115 ms/pair:

  | FI lanes | 1 | 2 | 3 | 4 |
  |---|---:|---:|---:|---:|
  | session.run | 112 ms | 235 ms | ~370 ms | 438 ms |
  | SR inference | 32 ms | 62 ms | ~66 ms | 108 ms |
  | RSS | 6.0 GB | 7.1 GB | 7.75 GB | 8.5 GB |

  The run is effectively serialised on the A40-24Q vGPU (compute plus 233 MB
  H2D and 100 MB D2H per pair). `utilization.gpu` reads only 30-40% here, which
  is misleading. Shipped presets use FI `num_workers: 2`; they are left
  unchanged because a full GPU may overlap copies with compute. On this vGPU,
  1 lane is as fast and uses about 1 GB less memory.
- The next real lever is keeping SR output on the GPU for FI (no D2H/H2D of 4K
  frames), or an FP16 RIFE model. Both are larger changes.

### Smaller items

- Decoder stage timer (`ebbf274`): it now times `next()`, so `avg_decode_ms`
  is real.
- ORT intra-op spinning off, or `with_intra_threads(1)`: no measurable change
  on x265 SR->FI (user 248-268 s incl. x265, wall within noise). Left at
  defaults. The ORT pools are 242 threads and about 9 s user per run.
- FI lanes burn about 30 s user per run while waiting for CUDA sync (spin
  wait). This is harmless while the CPU has headroom.
- FI preprocess takes 17 ms per 4K frame in isolation (`kbench` `fi_pre`,
  glibc or jemalloc) but about 67 ms in the pipeline. With jemalloc preloaded
  into the whole process tree it takes 28 ms. The cause is not isolated:
  child-process heap tunables did not reproduce it, and the pool hit rate is
  about 95%. It is not on the critical path while GPU-bound, so it was left as
  is.
- `inference_output_memory_info` and per-call IoBinding creation are cheap
  wrappers. They were left as is.
- target-cpu: still not set (see section 5).
- The first TensorRT session takes 6-7 s (CUDA/TRT init); later sessions take
  0.2-0.3 s. This is a per-process cost.
- TensorRT timing cache (`with_timing_cache` + a shared `<trt_cache>/timing`):
  tried and **not adopted**. SR-only engine build for a new resolution: 540p
  after populating the cache at 360p took 56.2 s, vs 57.9 s with no cache.
  The cache key includes layer input dimensions, so a new resolution reuses
  almost nothing. Script: `timing-cache-run.sh` in the harness dir.

Harness additions in `.omo/benchmarks/worker-perf-audit-20261001/`:

- `bench-build.sh <rev|WORKTREE> <name>`: release build into `bins/<name>`,
  with the local-only 256x256 NVENC probe patch; edit `S=` first.
- `summ.sh <run>`: one-line stage summary.
- `combined-nvenc-fi{1,2,4}.json`: FI lane sweep.
- `kbench/src/bin/fi_pre.rs`: isolated FI preprocess kernel.

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
