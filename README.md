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

## Docker

### Videnoa Worker

Use the prebuilt Docker Hub image on Linux x86-64 with a compatible NVIDIA GPU, driver, and [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html). CUDA and TensorRT are included; no local build or host CUDA installation is needed.

Place your models in `./models` and replace `$HOME/Videos` with your media directory:

```bash
mkdir -p ./models ./trt_cache
docker run --gpus all -p 3000:3000 \
  -v "$PWD/models:/app/models" \
  -v "$PWD/trt_cache:/app/trt_cache" \
  -v "$HOME/Videos:/data" \
  controlnet/videnoa:latest
```

Open `http://localhost:3000`; use `/data/...` paths for media. TensorRT builds engine caches on first use and reuses `./trt_cache` on subsequent runs.

To build locally instead, run the following and replace `controlnet/videnoa:latest` above with `videnoa`:

```bash
docker build -t videnoa .
```

### Videnoa Controller

Manage tasks across Videnoa workers; no GPU required. Replace `$HOME/Videos` with your media directory.

```bash
mkdir -p ./controller-data
docker run -d --name videnoa-controller \
  --user "$(id -u):$(id -g)" \
  -p 3001:3001 \
  -v "$PWD/controller-data:/workspace/data" \
  -v "$HOME/Videos:/media" \
  controlnet/videnoa-controller:latest
```

Open `http://localhost:3001`, set an administrator password, and add your Videnoa workers.
State persists in `./controller-data`; use `/media/...` paths for tasks.

View startup, task stages, Worker availability, and retries with `docker logs -f videnoa-controller`.
Logs default to `INFO`; add `-e RUST_LOG=warn,videnoa_controller=debug` to `docker run` for request and recovery diagnostics.

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

## Development setup

### Requirements

- Rust 1.98.0 (pinned in `rust-toolchain.toml`)
- Node.js 18+
- FFmpeg 4.4+
- NVIDIA GPU (required for CUDA or TensorRT acceleration)
- External ONNX Runtime shared library (required), TensorRT shared library (optional, recommended for speed)
- Dependency bundles are available in [misc files](https://github.com/ControlNet/videnoa/releases/tag/misc)

Rustup automatically selects the pinned toolchain in this checkout. To install it explicitly:

```bash
rustup toolchain install 1.98.0 --profile minimal --component clippy --component rustfmt
rustc --version
```

The version output should start with `rustc 1.98.0`.

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

### Optional access password

Configure an optional password in WebUI Settings. Controller workers support saved
access passwords. See [password access and recovery](docs/password-access.md) for
API authentication, session settings, public WebSocket behavior, and local recovery.
