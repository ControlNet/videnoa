#!/usr/bin/env bash
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
readonly SCRIPT="$REPO_ROOT/scripts/package_controller.ps1"

fail() {
  printf '[package_controller_windows_static_test][error] %s\n' "$*" >&2
  exit 1
}

[[ -f "$SCRIPT" ]] || fail "missing PowerShell packaging script"
grep -Fq "x86_64-pc-windows-msvc" "$SCRIPT" || fail "missing locked Windows target"
grep -Fq "windows-x86_64" "$SCRIPT" || fail "missing locked archive suffix"
grep -Fq "videnoa-controller.exe" "$SCRIPT" || fail "missing Windows executable entry"
grep -Fq "1980, 1, 1" "$SCRIPT" || fail "missing deterministic ZIP epoch"
grep -Fq "binary version mismatch" "$SCRIPT" || fail "missing binary version gate"
grep -Fq "forbidden GPU/runtime library" "$SCRIPT" || fail "missing PE runtime dependency gate"
grep -Fq "unexpected archive member or ordering" "$SCRIPT" || fail "missing exact layout gate"
grep -Eq "onnx|cuda|cudnn|nvinfer|tensorrt" "$SCRIPT" || fail "missing forbidden runtime gate"
if grep -Eq "package_dist|models\.zip|bin_win64|lib_win64" "$SCRIPT"; then
  fail "Controller packaging is coupled to the existing GPU distribution"
fi

printf '[package_controller_windows_static_test] PowerShell archive contracts present\n'
