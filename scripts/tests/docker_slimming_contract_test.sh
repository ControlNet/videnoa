#!/usr/bin/env bash
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
readonly WORKER_DOCKERFILE="$REPO_ROOT/Dockerfile"
readonly CONTROLLER_DOCKERFILE="$REPO_ROOT/Dockerfile.controller"
readonly DOCKERIGNORE="$REPO_ROOT/.dockerignore"
readonly WORKER_IMAGE="${1:-}"

fail() {
  printf '[docker-slimming-contract][error] %s\n' "$*" >&2
  exit 1
}

require_exact_line() {
  local file="$1"
  local line="$2"
  local description="$3"
  grep -Fqx -- "$line" "$file" || fail "$description"
}

[[ -f "$DOCKERIGNORE" ]] || fail '.dockerignore is missing'

for ignored_path in \
  '.git' \
  '.omo' \
  'target' \
  'data' \
  'lib' \
  'models' \
  'trt_cache' \
  '**/node_modules' \
  '**/dist' \
  '*.mkv' \
  '*.onnx' \
  '*.engine' \
  '*.plan' \
  '.env' \
  '.env.*'
do
  require_exact_line "$DOCKERIGNORE" "$ignored_path" ".dockerignore is missing $ignored_path"
done

if grep -Eq '(^|[[:space:]])strip([[:space:]]|$)' "$WORKER_DOCKERFILE"; then
  fail 'Worker release binary must retain symbols for production diagnostics'
fi
if grep -Eq '(^|[[:space:]])strip([[:space:]]|$)' "$CONTROLLER_DOCKERFILE"; then
  fail 'Controller release binary must retain symbols for production diagnostics'
fi

if grep -Fq 'COPY --from=builder /build/web/dist /app/web/dist' "$WORKER_DOCKERFILE"; then
  fail 'Worker runtime still duplicates the embedded WebUI'
fi

for removed_binary in mkvextract mkvinfo mkvmerge; do
  grep -Fq "/usr/bin/$removed_binary" "$WORKER_DOCKERFILE" \
    || fail "Worker runtime does not remove $removed_binary"
done

if grep -Fq '/usr/bin/mkvpropedit' "$WORKER_DOCKERFILE"; then
  fail 'Worker runtime removes required mkvpropedit'
fi

require_exact_line \
  "$WORKER_DOCKERFILE" \
  'FROM nvidia/cuda:12.8.0-base-ubuntu22.04 AS runtime' \
  'Worker runtime must use the release-aligned minimal CUDA base image'
require_exact_line \
  "$WORKER_DOCKERFILE" \
  'ARG ORT_VERSION=1.23.2' \
  'Worker runtime must use the release ONNX Runtime version'
require_exact_line \
  "$WORKER_DOCKERFILE" \
  'ARG TRT_VERSION=10.9.0.34' \
  'Worker runtime must use the RIFE-compatible TensorRT release'

for release_version_pin in \
  'ARG CUDA_NVRTC_VERSION=12.8.61-1' \
  'ARG CUBLAS_VERSION=12.8.3.14-1' \
  'ARG CUFFT_VERSION=11.3.3.41-1' \
  'ARG CURAND_VERSION=10.3.9.55-1' \
  'ARG CUDNN_VERSION=9.14.0.64-1' \
  'ARG NVFATBIN_VERSION=12.8.55-1' \
  'ARG NVJITLINK_VERSION=12.8.61-1'
do
  require_exact_line \
    "$WORKER_DOCKERFILE" \
    "$release_version_pin" \
    "Worker runtime is missing release version pin: $release_version_pin"
done

for required_cuda_package in \
  cuda-nvrtc-12-8 \
  libcublas-12-8 \
  libcufft-12-8 \
  libcurand-12-8 \
  libcudnn9-cuda-12 \
  libnvfatbin-12-8 \
  libnvjitlink-12-8
do
  grep -Eq "^[[:space:]]+$required_cuda_package(=|[[:space:]]|\\\\$)" "$WORKER_DOCKERFILE" \
    || fail "Worker runtime does not explicitly install $required_cuda_package"
done

if [[ -n "$WORKER_IMAGE" ]]; then
  docker run --rm --entrypoint /bin/sh "$WORKER_IMAGE" -c '
    set -eu
    test ! -e /app/web/dist
    test -x /usr/bin/mkvpropedit
    test ! -e /usr/bin/mkvextract
    test ! -e /usr/bin/mkvinfo
    test ! -e /usr/bin/mkvmerge
    grep -a -q '\.symtab' /usr/local/bin/videnoa
    mkvpropedit --version >/dev/null
    test -f /usr/local/lib/libonnxruntime.so.1.23.2
    test -f /usr/local/lib/libnvinfer_builder_resource.so.10.9.0

    require_package_version() {
      package="$1"
      expected_version="$2"
      actual_version="$(dpkg-query -W -f="\${Version}" "$package")"
      test "$actual_version" = "$expected_version"
    }

    require_package_version cuda-cudart-12-8 12.8.57-1
    require_package_version cuda-nvrtc-12-8 12.8.61-1
    require_package_version libcublas-12-8 12.8.3.14-1
    require_package_version libcufft-12-8 11.3.3.41-1
    require_package_version libcurand-12-8 10.3.9.55-1
    require_package_version libcudnn9-cuda-12 9.14.0.64-1
    require_package_version libnvfatbin-12-8 12.8.55-1
    require_package_version libnvjitlink-12-8 12.8.61-1

    for unused_cuda_package in \
      cuda-opencl-12-8 \
      libcusolver-12-8 \
      libcusparse-12-8 \
      libnccl2 \
      libnpp-12-8 \
      libcufile-12-8 \
      libnvjpeg-12-8
    do
      if dpkg-query -W "$unused_cuda_package" >/dev/null 2>&1; then
        echo "Unexpected CUDA package: $unused_cuda_package" >&2
        exit 1
      fi
    done

    for provider_library in \
      /usr/local/lib/libonnxruntime_providers_cuda.so \
      /usr/local/lib/libonnxruntime_providers_tensorrt.so
    do
      test -f "$provider_library"
      ! ldd "$provider_library" | grep -q "not found"
    done
  ' || fail 'Worker image content does not satisfy the slimming contract'

  docker run --rm --entrypoint /bin/sh "$WORKER_IMAGE" -c '
    set -eu
    videnoa --host 127.0.0.1 --port 3000 >/tmp/videnoa-smoke.log 2>&1 &
    server_pid=$!
    trap "kill $server_pid 2>/dev/null || true" EXIT
    attempt=0
    while [ "$attempt" -lt 40 ]; do
      if curl --fail --silent http://127.0.0.1:3000/api/health >/tmp/health.json; then
        break
      fi
      attempt=$((attempt + 1))
      sleep 0.5
    done
    test "$(cat /tmp/health.json)" = "{\"status\":\"ok\"}"
    curl --fail --silent http://127.0.0.1:3000/ | grep -q "<div id=\"root\"></div>"
  ' || fail 'Worker embedded WebUI HTTP smoke failed'
fi

printf '[docker-slimming-contract] PASS\n'
