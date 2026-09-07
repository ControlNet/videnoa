# TensorRT Batch Size 基准测试（2026-08-23）

## 结论

本轮不修改 Videnoa，不增加生产 `batch_size` 参数。

在 NVIDIA A30-24C、1080p 输入、AnimeJaNai V3 L1 Sharp HD x2 FP16、TensorRT 显式动态 profile（batch 1/2/4）下：

| Batch | 平均 FPS | 相对 batch 1 | FPS CV | ORT run ms/frame | 输出复制 ms/frame | 总计 ms/frame |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 19.452 | baseline | 0.951% | 22.364 | 29.047 | 51.410 |
| 2 | 19.787 | +1.720% | 1.073% | 21.692 | 28.850 | 50.542 |
| 4 | 20.163 | +3.655% | 1.250% | 20.689 | 28.911 | 49.600 |

Batch 4 的 GPU 利用率更高，但完整 inference-stage 吞吐只提高 3.66%。这不足以抵消生产实现所需的多帧缓冲、尾批处理、额外延迟、动态 profile/cache 管理、pipeline ordering 和更大瞬时内存等复杂度。

决策：**NO-GO**。保持当前 batch 1。后续优先优化占总耗时约 58% 的输出 materialization/host memory 路径，而不是先实现动态 batching。

## 测试对象

- GPU：NVIDIA A30-24C，24,576 MiB。
- 固定输入：`bench_per_commit_fixture_1080p_120f.mkv`。
- 输入 SHA256：`58bfbe8722bd801057aa907ae304bebcc6c2b4bafe979b1770c5e52207210fde`。
- 输入规格：1920x1080，FFV1，120 帧。
- 模型：`models/the_database_AnimeJaNaiV3L1_sharp_HD_x2_fp16_op17.onnx`。
- 模型 SHA256：`7666ddb0b9078e9fd47bf961e718a67715ad28894477be588d1fe69a9418a0a5`。
- 模型 IO：FP16 NCHW，动态 `batch_size`、`height`、`width`。
- ORT Rust crate：`ort = 2.0.0-rc.11`，与 Videnoa 完全一致。
- ONNX Runtime shared library：仓库 `lib/libonnxruntime.so`。
- TensorRT runtime：conda `anime` 环境中的 TensorRT 10.7 libraries。
- Benchmark binary SHA256：`38646504c7bbfd734f64385f5bdf9696d7b8a8c283d09b2aaf678e86a713fbab`。

## 隔离原则

- 未修改 `crates/`、`web/`、preset 或其他生产文件。
- Benchmark 位于 ignored 路径 `.omo/benchmarks/videnoa-trt-batch-bench/`。
- TensorRT cache 位于独立 ignored 路径 `.omo/benchmarks/videnoa-trt-batch-bench/trt-cache-profile-1-4/`。
- 未读取或写入共享 `trt_cache/`。
- 使用单一 engine/profile 比较 batch 1/2/4，避免每个 batch 使用独立最优 engine 导致不公平比较。

## TensorRT Profile

显式 profile：

```text
min: input:1x3x1080x1920
opt: input:2x3x1080x1920
max: input:4x3x1080x1920
```

配置：

- TensorRT EP，FP16 enabled。
- Engine cache enabled。
- Timing cache enabled。
- CUDA EP 作为注册链中的 fallback，但 TensorRT EP 注册失败会直接报错。
- Device ID 0。

生成的隔离 cache：

- `.engine`：483,788 bytes。
- `.profile`：42 bytes。
- `.timing`：68,001 bytes。
- 合计约 552 KiB。

## 方法

1. 用 FFmpeg 从固定 fixture 解码前 4 帧真实 RGB24 数据。
2. 在计时前一次性转换为 FP16 NCHW `[0,1]`。
3. 为 batch 1、2、4 构建形状 `[N,3,1080,1920]` 的输入 tensor。
4. 首次 `--build-only` 运行生成显式 profile engine；正式测量全部使用 warmed cache。
5. 每次正式运行先单帧生成 4 个 reference output。
6. batch 2/4 的每一帧输出与对应 batch-1 reference 全量比较。
7. 每个 batch 预热 2 次。
8. 每轮每个 batch 都处理 12 帧：batch 1 调用 12 次、batch 2 调用 6 次、batch 4 调用 3 次。
9. 三轮顺序交错为 `1,2,4`、`4,2,1`、`2,1,4`，降低固定顺序偏差。
10. 运行 3 次完整 warmed benchmark，报告 run-level 平均值与 CV。
11. `nvidia-smi` 每 100 ms 采集 GPU utilization 与 VRAM；runs 2/3 用每个 block 的 epoch timestamp 精确关联样本。

计时拆分：

- `run_ms`：同步 `session.run()`。
- `copy_ms`：将 4K FP16 output slice materialize 为 owned `Vec<f16>`。
- `total`：`run_ms + copy_ms`。

预处理不计时，因为本实验目标是单独判断 TensorRT inference batching 的收益。生产系统还会承担 batch 聚合与 frame representation 转换，因此这里的吞吐增益是偏乐观的上界，不是完整视频 pipeline 的保证。

## 正确性

所有正式运行均通过：

```text
VERIFY batch=2 values=49766400 max_abs_diff=0.00000000 mean_abs_diff=0.0000000000
VERIFY batch=4 values=99532800 max_abs_diff=0.00000000 mean_abs_diff=0.0000000000
```

Batch 2/4 与逐帧 batch-1 reference 在本模型和输入上 bit-exact。

## 三次 Warmed 结果

### Run 1

| Batch | FPS | run ms/frame | copy ms/frame | total ms/frame |
|---:|---:|---:|---:|---:|
| 1 | 19.625 | 21.967 | 28.987 | 50.955 |
| 2 | 20.022 | 21.123 | 28.822 | 49.944 |
| 4 | 20.448 | 20.045 | 28.859 | 48.904 |

### Run 2

| Batch | FPS | run ms/frame | copy ms/frame | total ms/frame |
|---:|---:|---:|---:|---:|
| 1 | 19.475 | 22.361 | 28.986 | 51.347 |
| 2 | 19.730 | 21.698 | 28.987 | 50.685 |
| 4 | 20.073 | 20.927 | 28.891 | 49.818 |

### Run 3

| Batch | FPS | run ms/frame | copy ms/frame | total ms/frame |
|---:|---:|---:|---:|---:|
| 1 | 19.257 | 22.763 | 29.167 | 51.929 |
| 2 | 19.609 | 22.256 | 28.740 | 50.996 |
| 4 | 19.969 | 21.094 | 28.984 | 50.077 |

## GPU 与 VRAM

Runs 2/3 timestamped block 汇总：

| Batch | GPU mean | GPU SD | GPU peak | VRAM mean | VRAM peak | Samples |
|---:|---:|---:|---:|---:|---:|---:|
| 1 | 24.895% | 6.294 | 32% | 2,589 MiB | 2,589 MiB | 38 |
| 2 | 26.632% | 4.277 | 32% | 2,589 MiB | 2,589 MiB | 38 |
| 4 | 27.821% | 3.077 | 33% | 2,589 MiB | 2,589 MiB | 39 |

解释：

- Batch 4 相对 batch 1 将平均 GPU utilization 提高约 2.93 个百分点。
- VRAM 在采样分辨率下没有表现出 batch 差异，三个 batch block 均为 2,589 MiB；ORT/TensorRT session 已在测量前预留内存，因此这不是逐次 tensor allocation 的精细测量。
- GPU 利用率随整个 benchmark 时间上升，说明存在温度、时钟、其他系统状态或采样平滑等顺序影响；交错顺序和三次独立运行用于降低该偏差。
- 吞吐排序在三次运行中均一致：batch 4 > batch 2 > batch 1，但收益始终小。

## 与生产路径的关系

当前 `SuperResInference::process_frame()` 还包含：

- `Vec<u16>` 到 `Vec<f16>` 转换。
- ndarray reshape。
- padding 检查/处理。
- `Tensor::from_array(input.clone())` 的输入 clone。
- `session.run()`。
- output `to_owned()`。
- `Vec<f16>` 到 `Vec<u16>` 转换。

本 benchmark 已包含 `session.run()` 和 output owned copy，但没有包含生产路径的 input clone、frame representation 转换、batch 聚合、尾批处理、下游拆批和 ordering 成本。因此 batch 4 的 +3.655% 应视为实现 batching 前可获得的乐观收益上界。

当前 output materialization 平均约 28.9 ms/frame，占 batch-1 measured stage 的约 56.5%；TensorRT run 约 22.4 ms/frame。Batching 只改善后者，无法改善占比更大的 host copy。

## Go/No-Go 标准与判断

建议只有在以下条件同时满足时才实现生产 batching：

- 完整 inference-stage 吞吐稳定提升至少 10%。
- 三次以上运行 CV 可控，且每次均改善。
- 输出正确性通过。
- VRAM 对目标 GPU 可接受。
- 完整视频 pipeline benchmark 仍保持明显收益，未被 batch buffering 和尾批成本抵消。

本轮：

- 正确性：通过。
- 稳定性：通过，CV 约 0.95% 至 1.25%。
- Batch 2 吞吐：仅 +1.72%，不通过。
- Batch 4 吞吐：仅 +3.66%，不通过。
- GPU utilization：改善，但不足以转化为显著吞吐。
- 完整 pipeline：未实现，因为 inference-only gate 已不满足。

最终判断：**NO-GO，不修改系统。**

## 后续优先级

下一轮更值得验证的方向：

1. 避免或减少 4K FP16 output 的 host materialization/copy。
2. IoBinding 或 device-resident tensor 在相邻 GPU stage 间传递。
3. 消除 `Tensor::from_array(input.clone())` 和 frame `u16`/`f16` 双向转换。
4. 在上述 host-memory 路径优化后，再重新测试 batch 2；只有收益超过 10% 才设计生产参数和流水线。

## 复现

构建：

```bash
export ORT_DYLIB_PATH="$PWD/lib/libonnxruntime.so"
export TRT_LIBS="$HOME/miniconda3/envs/anime/lib/python3.13/site-packages/tensorrt_libs"
export LD_LIBRARY_PATH="$TRT_LIBS:$PWD/lib:$HOME/miniconda3/envs/anime/lib:$LD_LIBRARY_PATH"
export PKG_CONFIG_PATH="$HOME/miniconda3/envs/anime/lib/pkgconfig:$PKG_CONFIG_PATH"
cargo build --release --manifest-path .omo/benchmarks/videnoa-trt-batch-bench/Cargo.toml
```

生成隔离 engine：

```bash
.omo/benchmarks/videnoa-trt-batch-bench/target/release/videnoa-trt-batch-bench \
  "$PWD/bench_per_commit_fixture_1080p_120f.mkv" \
  "$PWD/models/the_database_AnimeJaNaiV3L1_sharp_HD_x2_fp16_op17.onnx" \
  "$PWD/.omo/benchmarks/videnoa-trt-batch-bench/trt-cache-profile-1-4" \
  --build-only
```

正式运行：

```bash
.omo/benchmarks/videnoa-trt-batch-bench/target/release/videnoa-trt-batch-bench \
  "$PWD/bench_per_commit_fixture_1080p_120f.mkv" \
  "$PWD/models/the_database_AnimeJaNaiV3L1_sharp_HD_x2_fp16_op17.onnx" \
  "$PWD/.omo/benchmarks/videnoa-trt-batch-bench/trt-cache-profile-1-4"
```

原始证据：

- `.omo/benchmarks/videnoa-trt-batch-bench/build-only.log`
- `.omo/benchmarks/videnoa-trt-batch-bench/benchmark-run1.log`
- `.omo/benchmarks/videnoa-trt-batch-bench/benchmark-run2.log`
- `.omo/benchmarks/videnoa-trt-batch-bench/benchmark-run3.log`
- `.omo/benchmarks/videnoa-trt-batch-bench/gpu-samples-run1.csv`
- `.omo/benchmarks/videnoa-trt-batch-bench/gpu-samples-run2.csv`
- `.omo/benchmarks/videnoa-trt-batch-bench/gpu-samples-run3.csv`
