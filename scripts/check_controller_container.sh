#!/usr/bin/env bash
set -euo pipefail

readonly IMAGE="${1:-videnoa-controller:qa}"
readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly DOCKERFILE="$REPO_ROOT/Dockerfile.controller"
readonly MODE="${2:---all}"
readonly CONTAINER_NAME="videnoa-controller-smoke-$$"
WORK_DIR=""
PORT=""

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
  grep -Eq "$1" "$DOCKERFILE" || fail "Dockerfile.controller is missing $2"
}

check_source_contract() {
  [[ -f "$DOCKERFILE" ]] || fail "missing Dockerfile.controller"
  require_line '^FROM node:24-bookworm-slim AS controller-web$' 'the isolated frontend stage'
  require_line '^FROM rust:1\.83-bookworm AS controller-builder$' 'the Rust 1.83 builder stage'
  require_line '^FROM debian:bookworm-slim AS runtime$' 'the GPU-free runtime stage'
  require_line '^WORKDIR /workspace$' 'the workspace working directory'
  require_line '^USER 10001:10001$' 'the numeric non-root user'
  require_line '^ENTRYPOINT \["videnoa-controller"\]$' 'the Controller entrypoint'
  require_line '^CMD \["--host", "0\.0\.0\.0"\]$' 'the explicit container listener override'
  require_line '^HEALTHCHECK .*' 'the healthcheck'
  if grep -Eiq 'VOLUME|/etc/videnoa-controller|/var/lib/videnoa-controller|/mnt/input|/mnt/output|/run/secrets|videnoa-core|onnxruntime|tensor(rt)?|cuda|cudnn|/models' "$DOCKERFILE"; then
    fail "Dockerfile.controller contains a prepared-root, secret, GPU, or model contract"
  fi
  log "source contract: PASS"
}

check_image_contract() {
  local image_user image_entrypoint image_cmd image_workdir image_volumes linked_libraries packages
  image_user="$(docker image inspect --format '{{.Config.User}}' "$IMAGE")"
  [[ "$image_user" == '10001:10001' ]] || fail "image user is '$image_user'"
  image_entrypoint="$(docker image inspect --format '{{json .Config.Entrypoint}}' "$IMAGE")"
  [[ "$image_entrypoint" == '["videnoa-controller"]' ]] || fail "unexpected entrypoint: $image_entrypoint"
  image_cmd="$(docker image inspect --format '{{json .Config.Cmd}}' "$IMAGE")"
  [[ "$image_cmd" == '["--host","0.0.0.0"]' ]] || fail "unexpected command: $image_cmd"
  image_workdir="$(docker image inspect --format '{{.Config.WorkingDir}}' "$IMAGE")"
  [[ "$image_workdir" == '/workspace' ]] || fail "unexpected working directory: $image_workdir"
  image_volumes="$(docker image inspect --format '{{json .Config.Volumes}}' "$IMAGE")"
  [[ "$image_volumes" == 'null' ]] || fail "image declares legacy volumes: $image_volumes"
  docker run --rm --entrypoint /bin/sh "$IMAGE" -c \
    'test "$(id -u)" = 10001 && test "$(id -g)" = 10001 && test "$PWD" = /workspace && test ! -e /usr/local/bin/videnoa && ! command -v node && ! command -v npm' \
    || fail "runtime identity or minimal-image contract failed"
  linked_libraries="$(docker run --rm --entrypoint /usr/bin/ldd "$IMAGE" /usr/local/bin/videnoa-controller)"
  packages="$(docker run --rm --entrypoint /usr/bin/dpkg-query "$IMAGE" -W -f='${binary:Package}\n')"
  if printf '%s\n%s\n' "$linked_libraries" "$packages" | grep -Eiq 'onnx|tensor(rt)?|cuda|cudnn|nvidia'; then
    fail "runtime links or installs a prohibited GPU library"
  fi
  docker run --rm "$IMAGE" --help >/dev/null
  log "image contract: PASS"
}

new_fixture() {
  mkdir -p "$REPO_ROOT/data"
  WORK_DIR="$(mktemp -d "$REPO_ROOT/data/controller-container-XXXXXX")"
  mkdir -p "$WORK_DIR/media/input" "$WORK_DIR/media/output"
  printf 'synthetic container smoke media\n' >"$WORK_DIR/media/input/sample.mkv"
}

wait_for_health() {
  local attempt health
  for attempt in $(seq 1 30); do
    if health="$(curl --fail --silent --show-error "http://127.0.0.1:$PORT/api/health" 2>/dev/null)"; then
      [[ "$health" == '{"status":"ok"}' ]] || fail "unexpected health response: $health"
      return
    fi
    sleep 1
  done
  docker logs "$CONTAINER_NAME" >&2
  fail "container did not become healthy"
}

start_controller() {
  local mapping
  docker run -d --name "$CONTAINER_NAME" \
    --user "$(id -u):$(id -g)" \
    -p 127.0.0.1::3001 \
    --mount "type=bind,src=$WORK_DIR,dst=/workspace" \
    --workdir "${1:-/workspace}" \
    "$IMAGE" >/dev/null
  mapping="$(docker port "$CONTAINER_NAME" 3001/tcp)"
  PORT="${mapping##*:}"
  wait_for_health
}

setup_admin() {
  local password response status csrf
  password="container-smoke-$(tr -d '-' </proc/sys/kernel/random/uuid)"
  umask 077
  printf '{"password":"%s","password_confirmation":"%s"}\n' "$password" "$password" >"$WORK_DIR/setup.json"
  response="$(curl --silent --show-error --dump-header "$WORK_DIR/setup.headers" \
    --cookie-jar "$WORK_DIR/cookies" --output "$WORK_DIR/setup.response" --write-out '%{http_code}' \
    --header "Origin: http://127.0.0.1:$PORT" --header 'Content-Type: application/json' \
    --data-binary @"$WORK_DIR/setup.json" "http://127.0.0.1:$PORT/api/auth/setup")"
  status="$response"
  [[ "$status" == 200 ]] || fail "administrator setup returned HTTP $status"
  csrf="$(grep -i '^x-csrf-token:' "$WORK_DIR/setup.headers" | tr -d '\r' | cut -d' ' -f2-)"
  [[ -n "$csrf" ]] || fail "administrator setup omitted CSRF proof"
  [[ -s "$WORK_DIR/cookies" ]] || fail "administrator setup omitted session cookie"
  rm -f "$WORK_DIR/setup.json"
}

assert_setup_status() {
  local expected actual
  expected="$1"
  actual="$(curl --fail --silent --show-error "http://127.0.0.1:$PORT/api/auth/setup")"
  [[ "$actual" == "{\"initialized\":$expected}" ]] || fail "unexpected setup status: $actual"
}

check_runtime_happy() {
  new_fixture
  start_controller
  assert_setup_status false
  setup_admin
  assert_setup_status true
  [[ -s "$WORK_DIR/data/controller.toml" ]] || fail "Controller config was not created"
  [[ -s "$WORK_DIR/data/controller.sqlite3" ]] || fail "Controller database was not created"
  [[ "$(find "$WORK_DIR" -mindepth 1 -maxdepth 1 -printf '%f\n' | sort | paste -sd, -)" == 'cookies,data,media,setup.headers,setup.response' ]] \
    || fail "Controller created an unexpected workspace root entry"
  docker stop "$CONTAINER_NAME" >/dev/null
  docker rm "$CONTAINER_NAME" >/dev/null
  start_controller
  assert_setup_status true
  curl --fail --silent --show-error --cookie "$WORK_DIR/cookies" \
    "http://127.0.0.1:$PORT/api/auth/session" >/dev/null
  log "zero-config setup, workspace files, session, and restart persistence: PASS"
}

check_runtime_errors() {
  local status csrf
  new_fixture
  mkdir "$WORK_DIR/readonly"
  if docker run --rm --user "$(id -u):$(id -g)" \
    --mount "type=bind,src=$WORK_DIR/readonly,dst=/workspace,readonly" "$IMAGE" >/dev/null 2>&1; then
    fail "read-only workspace unexpectedly started"
  fi
  start_controller
  setup_admin
  csrf="$(grep -i '^x-csrf-token:' "$WORK_DIR/setup.headers" | tr -d '\r' | cut -d' ' -f2-)"
  status="$(curl --silent --show-error --output "$WORK_DIR/task.response" --write-out '%{http_code}' \
    --cookie "$WORK_DIR/cookies" --header "x-csrf-token: $csrf" \
    --header "Origin: http://127.0.0.1:$PORT" --header 'Content-Type: application/json' \
    --header 'Idempotency-Key: synthetic-container-private-state' \
    --data '{"input_path":"/workspace/data/controller.toml","output_path":"/workspace/media/output/out.mp4","workflow":"synthetic-test-workflow","source":"api","source_reference":null,"priority":0}' \
    "http://127.0.0.1:$PORT/api/tasks")"
  [[ "$status" == 400 ]] || fail "private-state task returned HTTP $status"
  log "read-only workspace and private-state task rejection: PASS"
}

check_external_media() {
  local status csrf
  new_fixture
  mkdir "$WORK_DIR/controller-workspace"
  start_controller /workspace/controller-workspace
  setup_admin
  csrf="$(grep -i '^x-csrf-token:' "$WORK_DIR/setup.headers" | tr -d '\r' | cut -d' ' -f2-)"
  status="$(curl --silent --show-error --output "$WORK_DIR/task.response" --write-out '%{http_code}' \
    --cookie "$WORK_DIR/cookies" --header "x-csrf-token: $csrf" \
    --header "Origin: http://127.0.0.1:$PORT" --header 'Content-Type: application/json' \
    --header 'Idempotency-Key: synthetic-container-external-media' \
    --data '{"input_path":"/workspace/media/input/sample.mkv","output_path":"/workspace/media/output/out.mp4","workflow":"synthetic-test-workflow","source":"api","source_reference":null,"priority":0}' \
    "http://127.0.0.1:$PORT/api/tasks")"
  [[ "$status" == 201 ]] || fail "external media task returned HTTP $status"
  [[ ! -e "$WORK_DIR/media/output/out.mp4" ]] || fail "intake created an incomplete final"
  [[ -s "$WORK_DIR/controller-workspace/data/controller.toml" ]] || fail "private configuration is missing"
  log "container-visible absolute media outside Controller workspace: PASS"
}

trap cleanup EXIT

case "$MODE" in
  --source) check_source_contract ;;
  --image) check_source_contract; check_image_contract ;;
  --happy) check_source_contract; check_image_contract; check_runtime_happy ;;
  --errors) check_source_contract; check_runtime_errors ;;
  --all) check_source_contract; check_image_contract; check_runtime_happy; cleanup; WORK_DIR=""; check_runtime_errors; cleanup; WORK_DIR=""; check_external_media ;;
  *) fail "usage: $0 [image] [--source|--image|--happy|--errors|--all]" ;;
esac
