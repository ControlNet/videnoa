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
  'FROM nvidia/cuda:12.6.3-base-ubuntu22.04 AS runtime' \
  'Worker runtime must use the minimal CUDA base image'

for required_cuda_package in \
  cuda-nvrtc-12-6 \
  libcublas-12-6 \
  libcufft-12-6 \
  libcurand-12-6 \
  libcudnn9-cuda-12 \
  libnvfatbin-12-6 \
  libnvjitlink-12-6
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

    for required_cuda_package in \
      cuda-cudart-12-6 \
      cuda-nvrtc-12-6 \
      libcublas-12-6 \
      libcufft-12-6 \
      libcurand-12-6 \
      libcudnn9-cuda-12 \
      libnvfatbin-12-6 \
      libnvjitlink-12-6
    do
      dpkg-query -W "$required_cuda_package" >/dev/null 2>&1
    done

    for unused_cuda_package in \
      cuda-opencl-12-6 \
      libcusolver-12-6 \
      libcusparse-12-6 \
      libnccl2 \
      libnpp-12-6 \
      libcufile-12-6 \
      libnvjpeg-12-6
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
