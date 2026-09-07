# TensorRT FP16 short-clip baseline (2026-08-23)

## Fixed inputs

- Git HEAD at capture: `5c46d9527a625f1db5a630ad06eb57f966d61caf`
- Binary: `target/release/videnoa`
  - SHA256: `68a10004b016f7f155af123bb45e68a8a207a6f44bf669367b57dfe79790614b`
- Preset: `presets/anime-2x-upscale.json`
  - SHA256: `5e1ba0359addd8478c65c706ac5526de98676dbaab9622ce2880ea7d5e6fa970`
  - AnimeJaNai x2 FP16, TensorRT, full-frame (`tile_size=0`), libx265 CRF 18, yuv420p10le
- Fixture: `bench_per_commit_fixture_1080p_120f.mkv` (ignored)
  - SHA256: `58bfbe8722bd801057aa907ae304bebcc6c2b4bafe979b1770c5e52207210fde`
  - FFV1, 1920x1080, yuv420p, 24000/1001 fps, 120 frames, 5.005 s, 56,806,131 bytes
  - Created from `1.mkv` at 300 seconds with:

```bash
ffmpeg -hide_banner -loglevel error -ss 300 -i "1.mkv" \
  -map 0:v:0 -frames:v 120 -an -sn -dn \
  -c:v ffv1 -level 3 -pix_fmt yuv420p \
  "bench_per_commit_fixture_1080p_120f.mkv" -y
```

- TensorRT cache: warmed `trt_cache/`, 4 files, 14,695,660 bytes. Every run logged `using existing cache`.
- GPU: NVIDIA A30-24C, driver 580.65.06, 24,576 MiB.

## Repeatable command

Use the same built binary, fixture, preset, cache, output codec, and 200 ms sampling cadence for every candidate. A candidate must record its new binary SHA256 before running.

```bash
export ORT_DYLIB_PATH="$PWD/lib/libonnxruntime.so"
export TRT_LIBS="$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs"
export LD_LIBRARY_PATH="$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:${LD_LIBRARY_PATH:-}"
export PKG_CONFIG_PATH="$HOME/miniconda3/envs/anime/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

for run_number in 1 2 3; do
  out="bench_per_commit_baseline_5c46d95/run${run_number}.mkv"
  log="bench_per_commit_baseline_5c46d95/run${run_number}.log"
  gpu="bench_per_commit_baseline_5c46d95/run${run_number}-gpu.csv"
  timing="bench_per_commit_baseline_5c46d95/run${run_number}-time.txt"
  rm -f "$out" "$log" "$gpu" "$timing"
  nvidia-smi \
    --query-gpu=timestamp,index,utilization.gpu,utilization.memory,memory.used,power.draw,clocks.current.sm,temperature.gpu \
    --format=csv,noheader,nounits -lms 200 > "$gpu" &
  sampler_pid=$!
  /usr/bin/time -v -o "$timing" \
    ./target/release/videnoa run presets/anime-2x-upscale.json \
      --input bench_per_commit_fixture_1080p_120f.mkv --output "$out" \
      > "$log" 2>&1
  exit_code=$?
  kill "$sampler_pid" 2>/dev/null || true
  wait "$sampler_pid" 2>/dev/null || true
  test "$exit_code" -eq 0 || exit "$exit_code"
  sleep 3
done
```

Probe every output with:

```bash
ffprobe -v error -count_frames -select_streams v:0 \
  -show_entries stream=codec_name,width,height,pix_fmt,r_frame_rate,avg_frame_rate,nb_read_frames \
  -show_entries format=duration,size -of json \
  bench_per_commit_baseline_5c46d95/run1.mkv
```

## Baseline results

`CLI fps` is the final application steady-state FPS after its two-frame warmup. `E2E fps` is 120 divided by `/usr/bin/time` wall time and includes TensorRT session initialization and finalization. GPU statistics include the complete sampler interval, including initialization and shutdown idle samples. VRAM is whole-device `memory.used`.

| Run | CLI fps | E2E fps | Wall s | GPU mean / p50 / p90 / p95 / max % | VRAM peak MiB | Pre / inference / post / encode ms per frame |
|---|---:|---:|---:|---|---:|---|
| 1 | 11.5 | 7.134 | 16.82 | 11.93 / 17 / 19 / 19 / 20 | 1881 | 15.8 / 88.6 / 54.2 / 25.2 |
| 2 | 11.5 | 7.177 | 16.72 | 12.02 / 17 / 19 / 19 / 20 | 1881 | 16.5 / 88.2 / 55.4 / 25.6 |
| 3 | 11.4 | 7.164 | 16.75 | 11.87 / 16 / 19 / 19 / 20 | 1881 | 16.0 / 88.9 / 56.6 / 28.8 |

Observed variance across three runs:

- CLI FPS: mean 11.467, sample SD 0.058, CV 0.50%, min-max span 0.87%.
- E2E FPS: mean 7.159, sample SD 0.022, CV 0.31%.
- Wall time: mean 16.763 s, sample SD 0.051 s, CV 0.31%.
- GPU mean: mean 11.940%, sample SD 0.079 percentage points, CV 0.66%.
- Inference: mean 88.567 ms/frame, sample SD 0.351 ms, CV 0.40%; implied capacity 11.291 fps.
- Preprocess: 16.100 ms/frame mean, CV 2.24%.
- Postprocess: 55.400 ms/frame mean, CV 2.17%.
- Encode: 26.533 ms/frame mean, CV 7.44%.
- Whole-device VRAM peak: 1881 MiB in all runs; idle start was 921 MiB, delta 960 MiB.
- Process RSS peak: mean 5149.7 MiB, CV 0.22%.
- Each GPU CSV has 83 samples. Active-sample GPU means were 13.75%, 13.86%, and 13.49%.

Every output was HEVC, 3840x2160, yuv420p10le, 2997/125 fps, 120 decoded frames, and 5.005 s. Each output was 1,396,193 bytes, although container hashes differed due to metadata.

## Raw artifacts and cleanup

- Raw directory: `bench_per_commit_baseline_5c46d95/` (ignored)
- Per run: `runN.log`, `runN-time.txt`, `runN-gpu.csv`, `runN.mkv`, `runN-ffprobe.json`
- Fixture: `bench_per_commit_fixture_1080p_120f.mkv` (ignored)
- Investigation ledger: `.debug-journal.md` (ignored, owned by the wider optimization investigation)
- No production files were modified by the benchmark work.
- Cleanup after the optimization campaign: remove `bench_per_commit_fixture_1080p_120f.mkv` and every `bench_per_commit_*` benchmark directory. Do not remove shared `trt_cache/` unless a cold-cache experiment explicitly requires it.

## Attribution rule

For candidate A/B comparisons, prioritize steady-state CLI FPS and inference ms/frame. Require three runs, matching output properties/frame count, warm cache confirmation, and no worse than roughly 1% regression/improvement before treating a change as distinct from the observed baseline throughput span. Report E2E FPS separately because session startup is about 3.5-3.8 seconds of this short fixture.

## Rejected candidates

- `TensorRef` borrowed input, binary `6dac761909b7de92b722b9a1436b1184e7a50bc6b9c0bedc284b2f3fe2eb0482`: inference increased to about 95.7 ms/frame and steady-state throughput fell to about 10.6 fps. Output correctness held; source was reverted.
- Native `Frame::NchwF16` payload (`Vec<half::f16>` instead of raw `Vec<u16>` bits), binary `1f1bc294705ba8b3ada7215c0bf45cafb3f11255cafeff195551392ea7b83492`: three warmed runs produced 10.7, 8.8, and 10.9 fps with 95.0, 115.7, and 93.5 ms/frame inference. All outputs matched the baseline format and frame count. The regression exceeded baseline variance in every run, so source was restored exactly to `HEAD`. Raw evidence is in ignored directory `bench_per_commit_nativef16_1f1bc29/`.
