#!/usr/bin/env bash
set -euo pipefail

IMAGE="${1:-videnoa-controller:qa}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DOCKERFILE="$REPO_ROOT/Dockerfile.controller"
MODE="${2:---all}"
CONTAINER_NAME="videnoa-controller-smoke-$$"
WORK_DIR=""

log() {
  printf '[controller-container] %s\n' "$*"
}

fail() {
  printf '[controller-container][error] %s\n' "$*" >&2
  exit 1
}

cleanup() {
  docker rm -f "$CONTAINER_NAME" >/dev/null 2>&1 || true
  if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
    rm -rf "$WORK_DIR"
  fi
}

require_line() {
  local pattern="$1"
  local description="$2"
  if ! grep -Eq "$pattern" "$DOCKERFILE"; then
    fail "Dockerfile.controller is missing $description"
  fi
}

check_source_contract() {
  [[ -f "$DOCKERFILE" ]] || fail "missing Dockerfile.controller"
  git -C "$REPO_ROOT" diff --quiet -- Dockerfile || fail "root GPU Dockerfile was modified"

  require_line '^FROM node:24-bookworm-slim AS controller-web$' 'the isolated frontend stage'
  require_line '^FROM rust:1\.83-bookworm AS controller-builder$' 'the Rust 1.83 builder stage'
  require_line '^FROM debian:bookworm-slim AS runtime$' 'the GPU-free runtime stage'
  require_line '^USER 10001:10001$' 'the numeric non-root user'
  require_line '^ENTRYPOINT \["videnoa-controller"\]$' 'the Controller entrypoint'
  require_line '^VOLUME \["/etc/videnoa-controller", "/var/lib/videnoa-controller", "/var/tmp/videnoa-controller", "/mnt/input", "/mnt/output"\]$' 'the persistent and NAS mount points'
  require_line '^HEALTHCHECK .*' 'the healthcheck'

  if grep -Eiq 'videnoa-core|onnxruntime|tensor(rt)?|cuda|cudnn|/models' "$DOCKERFILE"; then
    fail "Dockerfile.controller contains a prohibited core, GPU, or model dependency"
  fi
  log "source contract: PASS"
}

check_image_contract() {
  local image_user
  local image_entrypoint
  local image_cmd
  local image_volumes
  local linked_libraries
  local packages

  image_user="$(docker image inspect --format '{{.Config.User}}' "$IMAGE")"
  [[ "$image_user" == '10001:10001' ]] || fail "image user is '$image_user'"
  image_entrypoint="$(docker image inspect --format '{{json .Config.Entrypoint}}' "$IMAGE")"
  [[ "$image_entrypoint" == '["videnoa-controller"]' ]] || fail "unexpected entrypoint: $image_entrypoint"
  image_cmd="$(docker image inspect --format '{{json .Config.Cmd}}' "$IMAGE")"
  [[ "$image_cmd" == '["--config","/etc/videnoa-controller/controller.toml","--host","0.0.0.0"]' ]] || fail "unexpected command: $image_cmd"
  image_volumes="$(docker image inspect --format '{{json .Config.Volumes}}' "$IMAGE")"
  for volume in /etc/videnoa-controller /var/lib/videnoa-controller /var/tmp/videnoa-controller /mnt/input /mnt/output; do
    [[ "$image_volumes" == *"\"$volume\""* ]] || fail "image is missing volume $volume"
  done

  docker run --rm --entrypoint /bin/sh "$IMAGE" -c \
    'test "$(id -u)" = 10001 && test "$(id -g)" = 10001 && test ! -e /usr/local/bin/videnoa && test ! -e /app/models && ! command -v node && ! command -v npm' \
    || fail "runtime identity or Node-free contract failed"
  linked_libraries="$(docker run --rm --entrypoint /usr/bin/ldd "$IMAGE" /usr/local/bin/videnoa-controller)"
  packages="$(docker run --rm --entrypoint /usr/bin/dpkg-query "$IMAGE" -W -f='${binary:Package}\n')"
  if printf '%s\n%s\n' "$linked_libraries" "$packages" | grep -Eiq 'onnx|tensor(rt)?|cuda|cudnn|nvidia'; then
    fail "runtime links or installs a prohibited GPU library"
  fi
  local content_container
  content_container="$(docker create "$IMAGE" --help)"
  if docker export "$content_container" | tar -tf - | grep -Eiq '(^|/)(models|trt_cache)(/|$)|onnxruntime|tensor(rt)?|cuda|cudnn|nvidia'; then
    docker rm "$content_container" >/dev/null
    fail "runtime filesystem contains a prohibited GPU or model artifact"
  fi
  docker rm "$content_container" >/dev/null
  docker run --rm "$IMAGE" --help >/dev/null
  log "image contract: PASS"
}

write_config() {
  local path="$1"
  cat >"$path" <<'EOF'
[server]
host = "127.0.0.1"
port = 3001

[paths]
input_roots = ["/mnt/input"]
output_roots = ["/mnt/output"]
data_root = "/var/lib/videnoa-controller"
temp_root = "/var/tmp/videnoa-controller"

[auth]
password_hash_file = "/run/secrets/admin-password.phc"
secure_cookie = false
session_absolute_seconds = 86400
session_idle_seconds = 3600

[scheduler]
paused = false
default_compute_slots = 1
prefetch_per_worker = 1
max_concurrent_uploads = 1
max_concurrent_downloads = 1

[timeouts]
health_seconds = 10
poll_seconds = 5
transfer_seconds = 300

[retry]
initial_seconds = 1
maximum_seconds = 60
max_attempts = 5
EOF
}

wait_for_health() {
  local attempt
  local status
  for attempt in $(seq 1 30); do
    status="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{end}}' "$CONTAINER_NAME")"
    if [[ "$status" == healthy ]]; then
      return 0
    fi
    if [[ "$status" == unhealthy ]]; then
      docker logs "$CONTAINER_NAME" >&2
      fail "container became unhealthy"
    fi
    sleep 1
  done
  docker logs "$CONTAINER_NAME" >&2
  fail "container did not become healthy"
}

start_controller() {
  docker run -d --name "$CONTAINER_NAME" \
    -p 127.0.0.1::3001 \
    --mount "type=bind,src=$WORK_DIR/config,dst=/etc/videnoa-controller,readonly" \
    --mount "type=bind,src=$WORK_DIR/data,dst=/var/lib/videnoa-controller" \
    --mount "type=bind,src=$WORK_DIR/temp,dst=/var/tmp/videnoa-controller" \
    --mount "type=bind,src=$WORK_DIR/input,dst=/mnt/input,readonly" \
    --mount "type=bind,src=$WORK_DIR/output,dst=/mnt/output" \
    --mount "type=bind,src=$WORK_DIR/secrets/admin-password.phc,dst=/run/secrets/admin-password.phc,readonly" \
    "$IMAGE" >/dev/null
  wait_for_health
}

container_port() {
  docker port "$CONTAINER_NAME" 3001/tcp | sed 's/.*://'
}

check_runtime_happy() {
  local port
  local health
  local index

  WORK_DIR="$(mktemp -d -t videnoa-controller-container-XXXXXX)"
  mkdir -p "$WORK_DIR"/{config,data,temp,input,output,secrets}
  chmod 0777 "$WORK_DIR/data" "$WORK_DIR/temp" "$WORK_DIR/output"
  write_config "$WORK_DIR/config/controller.toml"
  local admin_secret
  admin_secret="$(tr -d '-' </proc/sys/kernel/random/uuid)$(tr -d '-' </proc/sys/kernel/random/uuid)"
  printf '%s\n' "$admin_secret" | docker run --rm -i "$IMAGE" hash-password >"$WORK_DIR/secrets/admin-password.phc"
  chmod 0644 "$WORK_DIR/secrets/admin-password.phc"

  start_controller
  port="$(container_port)"
  health="$(curl --fail --silent --show-error "http://127.0.0.1:$port/api/health")"
  [[ "$health" == '{"status":"ok"}' ]] || fail "unexpected health response: $health"
  index="$(curl --fail --silent --show-error "http://127.0.0.1:$port/tasks")"
  [[ "$index" == *'<div id="root"></div>'* ]] || fail "embedded SPA was not served"
  [[ -s "$WORK_DIR/data/controller.sqlite3" ]] || fail "Controller database was not persisted"
  docker stop "$CONTAINER_NAME" >/dev/null
  docker rm "$CONTAINER_NAME" >/dev/null

  start_controller
  [[ "$(curl --fail --silent --show-error "http://127.0.0.1:$(container_port)/api/health")" == '{"status":"ok"}' ]] \
    || fail "health failed after persistent-data restart"
  docker run --rm --entrypoint /bin/sh \
    --mount "type=bind,src=$WORK_DIR/data,dst=/var/lib/videnoa-controller" \
    --mount "type=bind,src=$WORK_DIR/temp,dst=/var/tmp/videnoa-controller" \
    "$IMAGE" -c 'touch /var/lib/videnoa-controller/write-test /var/tmp/videnoa-controller/write-test'
  log "runtime health, embedded SPA, writable mounts, and restart persistence: PASS"
}

expect_failure() {
  local name="$1"
  local expected="$2"
  shift 2
  local output
  if output="$("$@" 2>&1)"; then
    fail "$name unexpectedly succeeded"
  fi
  [[ "$output" == *"$expected"* ]] || fail "$name did not report '$expected': $output"
  if [[ -n "${admin_secret:-}" && "$output" == *"$admin_secret"* ]]; then
    fail "$name leaked authentication material"
  fi
  log "$name: PASS ($expected)"
}

check_runtime_errors() {
  WORK_DIR="$(mktemp -d -t videnoa-controller-errors-XXXXXX)"
  mkdir -p "$WORK_DIR"/{config,data,temp,input,output,secrets,readonly-data}
  chmod 0777 "$WORK_DIR/data" "$WORK_DIR/temp" "$WORK_DIR/output"
  write_config "$WORK_DIR/config/controller.toml"
  local admin_secret
  admin_secret="$(tr -d '-' </proc/sys/kernel/random/uuid)$(tr -d '-' </proc/sys/kernel/random/uuid)"
  printf '%s\n' "$admin_secret" | docker run --rm -i "$IMAGE" hash-password >"$WORK_DIR/secrets/admin-password.phc"
  chmod 0644 "$WORK_DIR/secrets/admin-password.phc"

  expect_failure 'missing config' 'configuration file is missing' docker run --rm "$IMAGE"
  expect_failure 'missing admin hash' 'password hash file is missing or invalid' docker run --rm \
    --mount "type=bind,src=$WORK_DIR/config,dst=/etc/videnoa-controller,readonly" \
    --mount "type=bind,src=$WORK_DIR/data,dst=/var/lib/videnoa-controller" \
    --mount "type=bind,src=$WORK_DIR/temp,dst=/var/tmp/videnoa-controller" \
    --mount "type=bind,src=$WORK_DIR/input,dst=/mnt/input,readonly" \
    --mount "type=bind,src=$WORK_DIR/output,dst=/mnt/output" "$IMAGE"
  expect_failure 'unwritable data' 'unable to open database file' docker run --rm \
    --mount "type=bind,src=$WORK_DIR/config,dst=/etc/videnoa-controller,readonly" \
    --mount "type=bind,src=$WORK_DIR/readonly-data,dst=/var/lib/videnoa-controller,readonly" \
    --mount "type=bind,src=$WORK_DIR/temp,dst=/var/tmp/videnoa-controller" \
    --mount "type=bind,src=$WORK_DIR/input,dst=/mnt/input,readonly" \
    --mount "type=bind,src=$WORK_DIR/output,dst=/mnt/output" \
    --mount "type=bind,src=$WORK_DIR/secrets/admin-password.phc,dst=/run/secrets/admin-password.phc,readonly" "$IMAGE"

  start_controller
  printf 'Authorization: Bearer %s\n' "$admin_secret" >"$WORK_DIR/auth-header"
  chmod 0600 "$WORK_DIR/auth-header"
  local response
  response="$(curl --silent --show-error --write-out $'\n%{http_code}' \
    --header @"$WORK_DIR/auth-header" \
    --header 'Content-Type: application/json' \
    --header 'Idempotency-Key: container-outside-root' \
    --data '{"input_path":"/etc/passwd.txt","output_path":"/mnt/output/out.mp4","workflow":"anime-2x","source":"api","source_reference":null,"priority":0}' \
    "http://127.0.0.1:$(container_port)/api/tasks")"
  [[ "$response" == *$'\n400' ]] || fail "outside-root request returned an unexpected status: $response"
  [[ "$response" == *'path is not available through configured roots'* ]] || fail "outside-root response was not explicit: $response"
  [[ "$response" != *"$admin_secret"* ]] || fail "outside-root response leaked authentication material"
  log "outside-root task: PASS (HTTP 400, configured-root error)"
}

trap cleanup EXIT

case "$MODE" in
  --source)
    check_source_contract
    ;;
  --image)
    check_source_contract
    check_image_contract
    ;;
  --happy)
    check_source_contract
    check_image_contract
    check_runtime_happy
    ;;
  --errors)
    check_source_contract
    check_runtime_errors
    ;;
  --all)
    check_source_contract
    check_image_contract
    check_runtime_happy
    cleanup
    WORK_DIR=""
    check_runtime_errors
    ;;
  *)
    fail "usage: $0 [image] [--source|--image|--happy|--errors|--all]"
    ;;
esac
