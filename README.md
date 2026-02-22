# videnoa (Video Enhancement Node-Orchestrated Automation)

<div align="center">
  <img src="https://img.shields.io/github/stars/ControlNet/videnoa?style=flat-square">
  <img src="https://img.shields.io/github/forks/ControlNet/videnoa?style=flat-square">
  <a href="https://github.com/ControlNet/videnoa/issues"><img src="https://img.shields.io/github/issues/ControlNet/videnoa?style=flat-square"></a>
  <img src="https://img.shields.io/github/license/ControlNet/videnoa?style=flat-square">
</div>

Node-based AI video enhancement pipeline automation, built in Rust and React.
Videnoa supports super-resolution (Real-ESRGAN / RealCUGAN) and frame interpolation (RIFE) with ONNX Runtime CUDA / TensorRT acceleration.

## Features

- **One workflow engine** for CLI, web server, and batch jobs
- **Super-resolution** (2x/4x) via Real-ESRGAN / RealCUGAN ONNX models
- **Frame interpolation** via RIFE (integer multipliers >= 2)
- **Web GUI** with node editor, presets, job history, and batch submission
- **CLI execution** with workflow parameter injection (`--param key=value`)
- **Jellyfin integration** through built-in workflow nodes
- **TensorRT support** with engine cache and optional IoBinding

## Requirements

- Rust 1.83+
- Node.js 18+
- FFmpeg 4.4+
- NVIDIA GPU (required for CUDA or TensorRT acceleration)
- External ONNX Runtime shared library (required), TensorRT shared library (optional, recommended for speed)
- Dependency bundles are available in [misc files](https://github.com/ControlNet/videnoa/releases/tag/misc)

## Development setup

### 1) Prepare runtime libraries and models

Download from [misc files](https://github.com/ControlNet/videnoa/releases/tag/misc), then place shared libraries in `lib/` and models in `models/`.

### 2) Build

```bash
cargo build --release --workspace
```

### 3) Run

#### 3.1) Start web server:

> First TensorRT run may take several minutes to build engine cache. Later runs are much faster.

```bash
./target/release/videnoa --host 0.0.0.0 --port 3000
```

#### 3.2) Run a workflow from CLI without GUI:

```bash
./target/release/videnoa run presets/anime-2x-upscale.json --input input.mkv --output output.mkv
./target/release/videnoa run <your_workflow.json> --param <key1>=<value1> --param <key2>=<value2> ...
```

#### 3.3) Run desktop app:

```bash
./target/release/videnoa-desktop
```


## Docker

Build image:

```bash
docker build -t videnoa .
```

Run server:

```bash
docker run --gpus all -p 3000:3000 \
  -v ./models:/app/models \
  -v ./trt_cache:/app/trt_cache \
  -v /path/to/media:/data \
  videnoa
```

## Configuration

Runtime config lives at `data/config.toml` (or `${VIDENOA_DATA_DIR}/config.toml`).

```toml
locale = "en"

[paths]
models_dir = "models"
trt_cache_dir = "trt_cache"
presets_dir = "presets"
workflows_dir = "data/workflows"

[server]
port = 3000
host = "0.0.0.0"

[performance]
profiling_enabled = false
```

CLI flags override config values (`--host`, `--port`, `--data-dir`).
