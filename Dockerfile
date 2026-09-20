# =============================================================================
# videnoa Dockerfile
# Multi-stage build: Rust compilation → NVIDIA CUDA runtime with ORT + TRT
#
# All runtime libraries (ONNX Runtime, TensorRT) are baked into the image.
# Mount models, the TensorRT cache, and /app/data for persistent Worker state.
# Mount host media separately when workflows need direct file access.
#
# Build:
#   docker build -t videnoa .
#
# Run server:
#   docker run -d --name videnoa --gpus all -p 3000:3000 \
#     -v "$PWD/models:/app/models" \
#     -v "$PWD/trt_cache:/app/trt_cache" \
#     -v "$PWD/data:/app/data" \
#     videnoa
#
# Run CLI:
#   docker run --gpus all \
#     -v "$PWD/models:/app/models" \
#     -v /path/to/media:/data \
#     videnoa videnoa run /app/presets/interpolation-2x.json \
#       -i /data/input.mkv -o /data/output.mkv
# =============================================================================

# ---------------------------------------------------------------------------
# Stage 1: Build the WebUI and Rust workspace
# ---------------------------------------------------------------------------
FROM node:24-bookworm-slim AS worker-web
WORKDIR /build/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-fund
COPY web/ ./
COPY presets/ /build/presets/
RUN npm run build

FROM rust:1.98.0-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        cmake \
        libssl-dev \
        libclang-dev \
        protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./

COPY --from=worker-web /build/web/dist/ web/dist/
COPY presets/ presets/

COPY crates/ crates/

ENV VIDENOA_WEB_PREBUILT=1
RUN cargo build --release --locked -p videnoa-app --bin videnoa

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

ARG TRT_VERSION=10.9.0.34
RUN pip install --no-cache-dir "tensorrt-cu12-libs==${TRT_VERSION}" \
    && mkdir /trt-lib \
    && cp /usr/local/lib/python3.12/site-packages/tensorrt_libs/libnvinfer.so.10 /trt-lib/ \
    && cp /usr/local/lib/python3.12/site-packages/tensorrt_libs/libnvinfer_plugin.so.10 /trt-lib/ \
    && cp /usr/local/lib/python3.12/site-packages/tensorrt_libs/libnvinfer_builder_resource.so.* /trt-lib/ \
    && cp /usr/local/lib/python3.12/site-packages/tensorrt_libs/libnvonnxparser.so.10 /trt-lib/

# ---------------------------------------------------------------------------
# Stage 4: Runtime image — minimal CUDA + cuDNN + bundled ORT + TRT
# ---------------------------------------------------------------------------
FROM nvidia/cuda:12.8.0-base-ubuntu22.04 AS runtime

# Keep these runtime pins aligned with the published release dependency set.
ARG CUDA_NVRTC_VERSION=12.8.61-1
ARG CUBLAS_VERSION=12.8.3.14-1
ARG CUFFT_VERSION=11.3.3.41-1
ARG CURAND_VERSION=10.3.9.55-1
ARG CUDNN_VERSION=9.14.0.64-1
ARG NVFATBIN_VERSION=12.8.55-1
ARG NVJITLINK_VERSION=12.8.61-1

RUN apt-get update && apt-get install -y --no-install-recommends \
        cuda-nvrtc-12-8=${CUDA_NVRTC_VERSION} \
        libcublas-12-8=${CUBLAS_VERSION} \
        libcufft-12-8=${CUFFT_VERSION} \
        libcurand-12-8=${CURAND_VERSION} \
        libcudnn9-cuda-12=${CUDNN_VERSION} \
        libnvfatbin-12-8=${NVFATBIN_VERSION} \
        libnvjitlink-12-8=${NVJITLINK_VERSION} \
        ffmpeg \
        mkvtoolnix \
        ca-certificates \
        curl \
    && rm -f \
        /usr/bin/mkvextract \
        /usr/bin/mkvinfo \
        /usr/bin/mkvmerge \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/videnoa /usr/local/bin/videnoa

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
