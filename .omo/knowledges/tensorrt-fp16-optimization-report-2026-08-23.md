# TensorRT FP16 超分性能优化实验报告

日期：2026-08-23
分支：`dev`
基线提交：`5c46d9527a625f1db5a630ad06eb57f966d61caf`

## 1. 结论摘要

本轮实验没有发现可保留的吞吐优化，两个候选改动均已从生产代码中完全回退：

1. 使用 ORT `TensorRef` 借用 FP16 输入，三次平均吞吐从 `11.467 fps` 降至 `11.000 fps`，吞吐 CV 从 `0.50%` 上升至 `7.10%`。该候选出现一次提升和两次明显回退，性能不稳定，未达到保留条件。
2. 将内部 `Frame::NchwF16` 数据从 `Vec<u16>` 改为 `Vec<half::f16>`，三次平均吞吐降至 `10.133 fps`，平均推理耗时升至 `101.400 ms/frame`，未达到保留条件。

所有九次基线及候选输出均通过正确性检查：HEVC、`3840x2160`、`yuv420p10le`、120 帧、5.005 秒。两个候选均只影响性能，没有观察到输出格式或帧数错误。

下一项优先实验应为：在现有单 inference worker、单 ORT Session 上实现保持输出顺序的全帧 FP16 batch size 2，并使用同一协议进行三次 A/B 测量。

## 2. 实验目标

工作负载为 AnimeJaNai V3 L1 Sharp HD x2 FP16 TensorRT 全帧超分。目标是在不改变输出正确性的前提下，提高稳态吞吐和 GPU 利用率。

候选改动必须满足以下保留条件：

- 使用同一个 release binary 完成三次 warmed-cache 运行。
- 每次输出必须通过 FFprobe 格式、分辨率、像素格式和帧数检查。
- 主要判据为稳态 CLI FPS 和 inference ms/frame。
- 提升幅度应稳定超过基线约 `1%`，避免把基线的正常方差误判为优化。
- 不接受只在单次运行中提升、但整体方差显著扩大的候选。

## 3. 固定测试条件

| 项目 | 值 |
|---|---|
| GPU | NVIDIA A30-24C，24,576 MiB |
| 驱动 | 580.65.06 |
| ONNX Runtime | 1.23.2 GPU |
| TensorRT | 10.7 |
| 预设 | `presets/anime-2x-upscale.json` |
| 模型路径 | AnimeJaNai V3 L1 Sharp HD x2 FP16 |
| 后端 | TensorRT，warmed engine cache |
| tile size | 0，全帧推理 |
| 输入 fixture | `bench_per_commit_fixture_1080p_120f.mkv` |
| 输入属性 | FFV1、1920x1080、120 帧、5.005 秒 |
| 输出编码 | libx265、CRF 18、`yuv420p10le` |
| GPU 采样 | `nvidia-smi`，200 ms 间隔 |

固定文件摘要：

- fixture SHA256：`58bfbe8722bd801057aa907ae304bebcc6c2b4bafe979b1770c5e52207210fde`
- preset SHA256：`5e1ba0359addd8478c65c706ac5526de98676dbaab9622ce2880ea7d5e6fa970`
- 基线 binary SHA256：`68a10004b016f7f155af123bb45e68a8a207a6f44bf669367b57dfe79790614b`

## 4. 测试方法

每个候选使用以下过程：

1. 在指定运行库环境下构建 release workspace。
2. 记录 candidate binary SHA256。
3. 使用同一 fixture、preset 和 warmed TensorRT cache 连续运行三次。
4. 每次保存应用日志、`/usr/bin/time -v` 数据、GPU 采样 CSV 和输出视频。
5. 使用 FFprobe 对每个输出执行 decoded frame count 和视频属性检查。
6. 将三次结果与基线均值、标准差和 CV 比较。

可重复运行命令：

```bash
export ORT_DYLIB_PATH="$PWD/lib/libonnxruntime.so"
export TRT_LIBS="$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs"
export LD_LIBRARY_PATH="$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:${LD_LIBRARY_PATH:-}"
export PKG_CONFIG_PATH="$HOME/miniconda3/envs/anime/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

./target/release/videnoa run presets/anime-2x-upscale.json \
  --input bench_per_commit_fixture_1080p_120f.mkv \
  --output output.mkv
```

## 5. 基线结果

| Run | 稳态 FPS | Wall time (s) | Preprocess | Inference | Postprocess | Encode |
|---|---:|---:|---:|---:|---:|---:|
| 1 | 11.5 | 16.82 | 15.8 ms | 88.6 ms | 54.2 ms | 25.2 ms |
| 2 | 11.5 | 16.72 | 16.5 ms | 88.2 ms | 55.4 ms | 25.6 ms |
| 3 | 11.4 | 16.75 | 16.0 ms | 88.9 ms | 56.6 ms | 28.8 ms |

基线统计：

| 指标 | 均值 | 样本标准差 | CV |
|---|---:|---:|---:|
| 稳态 FPS | 11.467 | 0.058 | 0.50% |
| Inference | 88.567 ms/frame | 0.351 ms | 0.40% |
| Wall time | 16.763 s | 0.051 s | 0.31% |
| 完整区间 GPU mean | 11.940% | 0.079 pct-pt | 0.66% |

其他基线数据：

- GPU max：20%。
- VRAM peak：1,881 MiB，三次相同。
- 输出均为 1,396,193 bytes；容器 SHA256 因元数据不同而不同。
- inference 是最长的单帧服务阶段，约束稳态吞吐。

## 6. 候选 A：ORT TensorRef 借用输入

### 6.1 改动

将直接 FP16 inference 输入从 `Tensor::from_array(input.clone())` 改为通过 `TensorRef` 借用 ndarray view，目标是消除一次输入 tensor clone。

Candidate binary SHA256：`6dac761909b7de92b722b9a1436b1184e7a50bc6b9c0bedc284b2f3fe2eb0482`

### 6.2 结果

| Run | 稳态 FPS | Wall time (s) | Preprocess | Inference | Postprocess | Encode |
|---|---:|---:|---:|---:|---:|---:|
| 1 | 10.6 | 17.90 | 15.6 ms | 95.7 ms | 56.3 ms | 22.0 ms |
| 2 | 11.9 | 16.63 | 17.6 ms | 85.4 ms | 55.6 ms | 24.3 ms |
| 3 | 10.5 | 17.85 | 15.7 ms | 96.4 ms | 54.7 ms | 34.2 ms |

| 指标 | 候选均值 | 相对基线 | 候选 CV |
|---|---:|---:|---:|
| 稳态 FPS | 11.000 | -4.07% | 7.10% |
| Inference | 92.500 ms/frame | +4.44% | 6.66% |
| Wall time | 17.460 s | +4.16% | 4.12% |
| 完整区间 GPU mean | 11.503% | -0.437 pct-pt | 3.58% |

### 6.3 判定

拒绝并回退。

第二次运行达到 `11.9 fps`，但第一次和第三次分别只有 `10.6` 和 `10.5 fps`。候选吞吐 CV 达 `7.10%`，远高于基线的 `0.50%`，同时三次均值下降 `4.07%`。这不是稳定、可归因的优化。

## 7. 候选 B：内部 Frame 使用原生 half::f16

### 7.1 改动

将内部 `Frame::NchwF16.data` 从原始 bit 表示 `Vec<u16>` 改为 `Vec<half::f16>`，并移除 super-resolution preprocess、inference、postprocess、frame interpolation 和 video output 边界上的 `to_bits()`、`from_bits()` 或 raw slice reinterpretation。

Candidate binary SHA256：`1f1bc294705ba8b3ada7215c0bf45cafb3f11255cafeff195551392ea7b83492`

### 7.2 结果

| Run | 稳态 FPS | Wall time (s) | Preprocess | Inference | Postprocess | Encode |
|---|---:|---:|---:|---:|---:|---:|
| 1 | 10.7 | 17.59 | 17.4 ms | 95.0 ms | 55.7 ms | 28.8 ms |
| 2 | 8.8 | 20.16 | 17.3 ms | 115.7 ms | 57.3 ms | 27.8 ms |
| 3 | 10.9 | 17.35 | 15.4 ms | 93.5 ms | 53.8 ms | 32.2 ms |

| 指标 | 候选均值 | 相对基线 | 候选 CV |
|---|---:|---:|---:|
| 稳态 FPS | 10.133 | -11.63% | 11.44% |
| Inference | 101.400 ms/frame | +14.49% | 12.24% |
| Wall time | 18.367 s | +9.57% | 8.48% |
| 完整区间 GPU mean | 10.917% | -1.023 pct-pt | 8.46% |

### 7.3 判定

拒绝并回退。

三次运行都没有稳定超过基线门槛；最快一次 `10.9 fps` 仍比基线均值低约 `4.94%`。平均 inference 耗时增加 `14.49%`，且第二次运行出现明显长尾。移除显式 bit 转换没有转化为端到端收益。

## 8. 输出正确性

基线、TensorRef 候选和原生 `half::f16` 候选共九个输出均满足：

| 属性 | 结果 |
|---|---|
| Codec | HEVC |
| Width x Height | 3840x2160 |
| Pixel format | `yuv420p10le` |
| Frame rate | `2997/125` |
| Decoded frames | 120 |
| Duration | 5.005 s |
| File size | 1,396,193 bytes |

因此两个候选的拒绝原因均为性能，而不是正确性失败。

## 9. 原因分析

当前 FP16 micro-stage 的核心限制仍然是 batch 1 的同步推理：

- 单 inference worker 持有单个 `Arc<Mutex<Session>>`。
- 每帧调用同步 `session.run()`。
- pipeline channel 深度允许 CPU 阶段与 GPU inference 重叠，但不会产生并发 GPU submission。
- baseline inference `88.567 ms/frame` 对应理论容量约 `11.29 fps`，与实际稳态吞吐接近。
- 既有长视频 null sink 结果仅比 libx265 输出快约 `0.3 fps`，编码不是主瓶颈。

两个候选只改变 host-side tensor ownership 或表示方式，没有改变 GPU submission 形态。实验结果表明，这些局部 copy 优化不足以突破同步 batch-1 inference 上限，并可能触发 ORT/allocator 行为差异导致更大波动。

## 10. 下一步实验

优先测试保持顺序的全帧 FP16 micro-batching，第一档 batch size 为 2：

1. 保持单 inference worker 和单 ORT Session，避免多 Session TensorRT engine 与 VRAM 放大。
2. 在 inference stage 聚合连续两帧，形成 `[2, 3, H, W]` 输入。
3. 一次 `session.run()` 后按原始 frame index 拆分输出，严格保持 encoder 顺序。
4. 对奇数尾帧采用 batch 1，不补虚假帧。
5. 记录 batch-1 baseline 与 batch-2 candidate 的稳态 FPS、每帧 inference、GPU mean、GPU active mean、VRAM peak 和输出帧顺序。
6. 沿用三次 warmed-cache、正确性验证和大于约 `1%` 的保留门槛。

风险点：模型和 TensorRT cache profile 是否接受 dynamic batch 2、batch 合并的额外 host copy、VRAM 峰值和尾帧处理。若模型输入 batch 维固定为 1，则应先导出或构建支持 batch 2 的 profile，而不是并行调用同一个 Session。

## 11. 证据位置与清理

原始视频、日志和 GPU CSV 体积较大，不提交 Git；本地证据位于：

- `bench_per_commit_baseline_5c46d95/`
- `bench_per_commit_tensorref_6dac761/`
- `bench_per_commit_nativef16_1f1bc29/`
- `bench_per_commit_fixture_1080p_120f.mkv`

这些路径由 `.gitignore` 排除。完成整个优化 campaign 后可以删除 benchmark 目录和 fixture；共享 `trt_cache/` 不应随意清理，否则下一次实验会重新编译 TensorRT engine，破坏 warmed-cache 可比性。
