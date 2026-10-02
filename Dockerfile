# =============================================================================
# videnoa Dockerfile
# Multi-stage build: Rust compilation → NVIDIA CUDA runtime with ORT + TRT
#
# All runtime libraries (ONNX Runtime, TensorRT) and the release media tools
# bundle (FFmpeg 8.1, ffprobe, mkvpropedit in /app/bin) are baked into the image.
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
# Stage 4: Download the release media tools bundle (FFmpeg 8.1, ffprobe,
# mkvpropedit). This is the same misc-release asset the Linux release archive
# ships (scripts/package_dist.sh, scripts/setup_dev_media_tools.sh), so the
# image encodes with the same FFmpeg build instead of Ubuntu's FFmpeg 4.4.
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS media-tools

RUN apt-get update && apt-get install -y --no-install-recommends wget ca-certificates unzip \
    && rm -rf /var/lib/apt/lists/*

# Pinned to a specific upload of the mutable `misc` release asset; bump the
# checksum together with the asset. Only mkvpropedit is used, so the other
# MKVToolNix executables are dropped after the bundle checksums are verified.
ARG MEDIA_TOOLS_URL=https://github.com/ControlNet/videnoa/releases/download/misc/bin_linux64.zip
ARG MEDIA_TOOLS_SHA256=6d1944351a5d4d236eae8930dd6a68a4d7da73228e5c6c5657cbb3967cdeb1a9
RUN wget -q --tries=4 --timeout=120 -O /tmp/bin_linux64.zip "${MEDIA_TOOLS_URL}" \
    && echo "${MEDIA_TOOLS_SHA256}  /tmp/bin_linux64.zip" | sha256sum -c - \
    && unzip -q /tmp/bin_linux64.zip -d /tmp/media-tools \
    && test -x /tmp/media-tools/bin/ffmpeg \
    && (cd /tmp/media-tools/bin && sha256sum --quiet -c SHA256SUMS) \
    && mv /tmp/media-tools/bin /media-tools \
    && rm -rf /tmp/bin_linux64.zip /tmp/media-tools \
    && rm -f \
        /media-tools/.mkvtoolnix-101.0/usr/bin/mkvextract \
        /media-tools/.mkvtoolnix-101.0/usr/bin/mkvinfo \
        /media-tools/.mkvtoolnix-101.0/usr/bin/mkvmerge \
        /media-tools/.mkvtoolnix-101.0/usr/bin/mkvtoolnix-gui

# ---------------------------------------------------------------------------
# Stage 5: Runtime image — minimal CUDA + cuDNN + bundled ORT + TRT
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
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/videnoa /usr/local/bin/videnoa

COPY --from=ort-download /ort-lib/ /usr/local/lib/
COPY --from=trt-download /trt-lib/ /usr/local/lib/

RUN ldconfig

# runtime::command_for resolves ffmpeg/ffprobe/mkvpropedit from <cwd>/bin
# (WORKDIR /app) before PATH; PATH covers runs with a different working dir.
COPY --from=media-tools /media-tools/ /app/bin/
ENV PATH="/app/bin:${PATH}"

RUN mkdir -p /app/models /app/trt_cache /app/config /app/presets /data

COPY presets/ /app/presets/

ENV RUST_LOG=info

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:3000/api/health || exit 1

ENTRYPOINT []
CMD ["videnoa"]
