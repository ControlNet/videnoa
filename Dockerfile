# =============================================================================
# videnoa Dockerfile
# Multi-stage build: Rust compilation → NVIDIA CUDA runtime with ORT + TRT
#
# All runtime libraries (ONNX Runtime, TensorRT) are baked into the image.
# Users only need to mount models and media directories.
#
# Build:
#   docker build -t videnoa .
#
# Run server:
#   docker run --gpus all -p 3000:3000 \
#     -v ./models:/app/models \
#     -v ./trt_cache:/app/trt_cache \
#     videnoa
#
# Run CLI:
#   docker run --gpus all \
#     -v ./models:/app/models \
#     -v /path/to/media:/data \
#     videnoa videnoa run /app/presets/interpolation-2x.json \
#       -i /data/input.mkv -o /data/output.mkv
# =============================================================================

# ---------------------------------------------------------------------------
# Stage 1: Build the Rust workspace
# ---------------------------------------------------------------------------
FROM rust:1.83-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        libclang-dev \
        nodejs \
        npm \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock

COPY crates/core/Cargo.toml crates/core/Cargo.toml
COPY crates/app/Cargo.toml crates/app/Cargo.toml
COPY crates/desktop/Cargo.toml crates/desktop/Cargo.toml

RUN mkdir -p crates/core/src && echo "" > crates/core/src/lib.rs \
    && mkdir -p crates/app/src && echo "" > crates/app/src/lib.rs \
    && echo "fn main() {}" > crates/app/src/main.rs \
    && mkdir -p crates/desktop/src && echo "fn main() {}" > crates/desktop/src/main.rs

RUN cargo build --release -p videnoa-app --bin videnoa 2>/dev/null || true

COPY web/ web/
RUN cd web && npm install && npm run build

RUN rm -rf crates/*/src

COPY crates/ crates/

RUN cargo build --release -p videnoa-app --bin videnoa

# ---------------------------------------------------------------------------
# Stage 2: Download ONNX Runtime GPU
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS ort-download

RUN apt-get update && apt-get install -y --no-install-recommends wget ca-certificates \
    && rm -rf /var/lib/apt/lists/*

ARG ORT_VERSION=1.23.2
RUN wget -q "https://github.com/microsoft/onnxruntime/releases/download/v${ORT_VERSION}/onnxruntime-linux-x64-gpu-${ORT_VERSION}.tgz" \
    && tar xzf "onnxruntime-linux-x64-gpu-${ORT_VERSION}.tgz" \
    && mv "onnxruntime-linux-x64-gpu-${ORT_VERSION}/lib" /ort-lib \
    && rm -rf "onnxruntime-linux-x64-gpu-${ORT_VERSION}"*

# ---------------------------------------------------------------------------
# Stage 3: Download TensorRT runtime libs
# ---------------------------------------------------------------------------
FROM python:3.12-slim-bookworm AS trt-download

ARG TRT_VERSION=10.7.0
RUN pip install --no-cache-dir "tensorrt-cu12-libs==${TRT_VERSION}" \
    && mkdir /trt-lib \
    && cp /usr/local/lib/python3.12/site-packages/tensorrt_libs/libnvinfer.so.10 /trt-lib/ \
    && cp /usr/local/lib/python3.12/site-packages/tensorrt_libs/libnvinfer_plugin.so.10 /trt-lib/ \
    && cp /usr/local/lib/python3.12/site-packages/tensorrt_libs/libnvonnxparser.so.10 /trt-lib/

# ---------------------------------------------------------------------------
# Stage 4: Runtime image — CUDA + cuDNN + bundled ORT + TRT
# ---------------------------------------------------------------------------
FROM nvidia/cuda:12.6.3-cudnn-runtime-ubuntu22.04 AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        ffmpeg \
        mkvtoolnix \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/videnoa /usr/local/bin/videnoa
COPY --from=builder /build/web/dist /app/web/dist

COPY --from=ort-download /ort-lib/ /usr/local/lib/
COPY --from=trt-download /trt-lib/ /usr/local/lib/

RUN ldconfig

RUN mkdir -p /app/models /app/trt_cache /app/config /app/presets /data

COPY presets/ /app/presets/

ENV RUST_LOG=info

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:3000/api/health || exit 1

ENTRYPOINT []
CMD ["videnoa"]
