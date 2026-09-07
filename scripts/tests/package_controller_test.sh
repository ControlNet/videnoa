#!/usr/bin/env bash
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
readonly PACKAGE_SCRIPT="$REPO_ROOT/scripts/package_controller.sh"
readonly VERSION="$(awk -F '"' '/^version = "/ { print $2; exit }' "$REPO_ROOT/Cargo.toml")"
readonly ROOT_NAME="videnoa-controller-v${VERSION}-linux-x86_64"
TEMPORARY=""

fail() {
  printf '[package_controller_test][error] %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$TEMPORARY" && -d "$TEMPORARY" ]]; then
    rm -rf "$TEMPORARY"
  fi
}

assert_fails_with() {
  local expected="$1"
  shift
  local output
  if output="$("$@" 2>&1)"; then
    fail "command unexpectedly succeeded: $*"
  fi
  [[ "$output" == *"$expected"* ]] || fail "failure did not contain '$expected': $output"
}

make_binary() {
  local path="$1"
  local version="$2"
  local source="${path}.c"
  cat >"$source" <<EOF
#include <stdio.h>
#include <string.h>
int main(int argc, char **argv) {
  if (argc == 2 && strcmp(argv[1], "--version") == 0) {
    puts("videnoa-controller ${version}");
    return 0;
  }
  if (argc == 2 && strcmp(argv[1], "--help") == 0) {
    puts("Controller help");
    return 0;
  }
  return 2;
}
EOF
  cc -O2 -o "$path" "$source"
}

main() {
  [[ -x "$PACKAGE_SCRIPT" ]] || fail "missing executable packaging script: $PACKAGE_SCRIPT"
  command -v cc >/dev/null 2>&1 || fail "cc is required for the test binary fixture"
  TEMPORARY="$(mktemp -d -t videnoa-controller-package-test-XXXXXX)"
  trap cleanup EXIT
  local temporary="$TEMPORARY"

  local binary="$temporary/videnoa-controller"
  make_binary "$binary" "$VERSION"

  mkdir "$temporary/out-a" "$temporary/out-b"
  "$PACKAGE_SCRIPT" --binary-path "$binary" --output-dir "$temporary/out-a"
  "$PACKAGE_SCRIPT" --binary-path "$binary" --output-dir "$temporary/out-b"

  local archive_a="$temporary/out-a/${ROOT_NAME}.tar.gz"
  local archive_b="$temporary/out-b/${ROOT_NAME}.tar.gz"
  [[ -f "$archive_a" ]] || fail "missing archive: $archive_a"
  [[ "$(sha256sum "$archive_a" | cut -d' ' -f1)" == "$(sha256sum "$archive_b" | cut -d' ' -f1)" ]] \
    || fail "identical inputs produced different archive checksums"

  local expected_manifest actual_manifest
  expected_manifest="$(printf '%s\n' \
    "$ROOT_NAME/" \
    "$ROOT_NAME/LICENSE" \
    "$ROOT_NAME/README-controller.md" \
    "$ROOT_NAME/controller.example.toml" \
    "$ROOT_NAME/videnoa-controller")"
  actual_manifest="$(tar -tzf "$archive_a")"
  [[ "$actual_manifest" == "$expected_manifest" ]] || fail "unexpected archive manifest: $actual_manifest"
  "$PACKAGE_SCRIPT" --verify-archive "$archive_a"

  make_binary "$temporary/wrong-version" "9.9.9"
  assert_fails_with "binary version mismatch" \
    "$PACKAGE_SCRIPT" --binary-path "$temporary/wrong-version" --output-dir "$temporary/out-a" --force

  mkdir -p "$temporary/missing-source"
  cp "$REPO_ROOT/Cargo.toml" "$temporary/missing-source/Cargo.toml"
  cp "$REPO_ROOT/controller.example.toml" "$temporary/missing-source/controller.example.toml"
  cp "$REPO_ROOT/LICENSE" "$temporary/missing-source/LICENSE"
  assert_fails_with "missing required file: README-controller.md" \
    "$PACKAGE_SCRIPT" --source-dir "$temporary/missing-source" --binary-path "$binary" \
    --output-dir "$temporary/out-a" --force

  mkdir -p "$temporary/injected/$ROOT_NAME/models"
  tar -xzf "$archive_a" -C "$temporary/injected"
  printf 'forbidden\n' >"$temporary/injected/$ROOT_NAME/models/model.onnx"
  mkdir "$temporary/forbidden-out"
  local forbidden_archive="$temporary/forbidden-out/${ROOT_NAME}.tar.gz"
  tar -czf "$forbidden_archive" -C "$temporary/injected" "$ROOT_NAME"
  assert_fails_with "unexpected archive member" \
    "$PACKAGE_SCRIPT" --verify-archive "$forbidden_archive"

  printf '[package_controller_test] all archive contracts passed\n'
}

main "$@"
