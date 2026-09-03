#!/usr/bin/env bash
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
readonly DEFAULT_SOURCE_DIR="$(cd "$SCRIPT_DIR/.." && pwd -P)"
readonly DEFAULT_TARGET="x86_64-unknown-linux-gnu"
readonly ARCHIVE_EPOCH="${SOURCE_DATE_EPOCH:-0}"

SOURCE_DIR="$DEFAULT_SOURCE_DIR"
OUTPUT_DIR="$PWD"
TARGET="$DEFAULT_TARGET"
BINARY_PATH=""
VERIFY_ARCHIVE=""
FORCE="false"
WORK_DIR=""

usage() {
  cat <<'EOF'
Package the standalone Videnoa Controller Linux archive.

Usage:
  scripts/package_controller.sh [options]
  scripts/package_controller.sh --verify-archive <archive.tar.gz>

Options:
  --target <triple>          Rust target (default: x86_64-unknown-linux-gnu)
  --source-dir <path>        Repository root (default: script parent)
  --output-dir <path>        Archive output directory (default: current directory)
  --binary-path <path>       Package an already-built Controller binary
  --verify-archive <path>    Verify an existing Linux Controller archive
  --force                    Replace an existing archive
  -h, --help                 Show this help message
EOF
}

log() {
  printf '[package_controller] %s\n' "$*"
}

die() {
  printf '[package_controller][error] %s\n' "$*" >&2
  exit 1
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

cleanup() {
  if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
    rm -rf "$WORK_DIR"
  fi
}

workspace_version() {
  local manifest="$1/Cargo.toml"
  [[ -f "$manifest" ]] || die "missing required file: Cargo.toml"
  local version
  version="$(awk -F '"' '/^version = "/ { print $2; exit }' "$manifest")"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.-]+)?$ ]] \
    || die "workspace version is missing or invalid in Cargo.toml"
  printf '%s\n' "$version"
}

validate_source_files() {
  local required
  for required in controller.example.toml README-controller.md LICENSE; do
    [[ -f "$SOURCE_DIR/$required" ]] || die "missing required file: $required"
  done
}

validate_linux_binary() {
  local binary="$1"
  local version="$2"
  [[ -f "$binary" && -x "$binary" ]] || die "missing executable Controller binary: $binary"
  file "$binary" | grep -q 'ELF 64-bit.*x86-64' \
    || die "Controller binary is not a Linux x86_64 executable: $binary"
  local actual_version
  actual_version="$($binary --version 2>&1)" \
    || die "Controller binary version command failed: $binary --version"
  [[ "$actual_version" == "videnoa-controller $version" ]] \
    || die "binary version mismatch: expected 'videnoa-controller $version', got '$actual_version'"
  if command -v ldd >/dev/null 2>&1; then
    local dependencies
    dependencies="$(ldd "$binary" 2>&1 || true)"
    if grep -Eiq '(onnxruntime|libort|cuda|cudnn|nvinfer|tensorrt)' <<<"$dependencies"; then
      die "Controller binary links a forbidden GPU/runtime library"
    fi
  fi
}

expected_manifest() {
  local root="$1"
  printf '%s\n' \
    "$root/" \
    "$root/LICENSE" \
    "$root/README-controller.md" \
    "$root/controller.example.toml" \
    "$root/videnoa-controller"
}

verify_archive() {
  local archive="$1"
  [[ -f "$archive" ]] || die "archive does not exist: $archive"
  local filename version root actual expected
  filename="$(basename "$archive")"
  [[ "$filename" =~ ^videnoa-controller-v(.+)-linux-x86_64\.tar\.gz$ ]] \
    || die "archive filename does not match the Linux Controller contract: $filename"
  version="${BASH_REMATCH[1]}"
  root="videnoa-controller-v${version}-linux-x86_64"
  actual="$(tar -tzf "$archive")" || die "unable to list archive: $archive"
  expected="$(expected_manifest "$root")"
  [[ "$actual" == "$expected" ]] || die "unexpected archive member or ordering"
  if grep -Eiq '(^|/)(models?|lib|bin|target|trt_cache|controller-web|dist|\.env|.*\.(onnx|engine|plan|dll|so([.][0-9]+)*|dylib|pdb|key|pem))(/|$)' <<<"$actual"; then
    die "archive contains forbidden GPU/model/runtime/cache/secret content"
  fi
  log "archive layout verified: $archive"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target)
      [[ $# -ge 2 ]] || die "missing value for --target"
      TARGET="$2"
      shift 2
      ;;
    --source-dir)
      [[ $# -ge 2 ]] || die "missing value for --source-dir"
      SOURCE_DIR="$2"
      shift 2
      ;;
    --output-dir)
      [[ $# -ge 2 ]] || die "missing value for --output-dir"
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --binary-path)
      [[ $# -ge 2 ]] || die "missing value for --binary-path"
      BINARY_PATH="$2"
      shift 2
      ;;
    --verify-archive)
      [[ $# -ge 2 ]] || die "missing value for --verify-archive"
      VERIFY_ARCHIVE="$2"
      shift 2
      ;;
    --force)
      FORCE="true"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

require_cmd tar
require_cmd gzip

if [[ -n "$VERIFY_ARCHIVE" ]]; then
  verify_archive "$VERIFY_ARCHIVE"
  exit 0
fi

[[ "$TARGET" == "$DEFAULT_TARGET" ]] || die "unsupported Linux target: $TARGET"
SOURCE_DIR="$(cd "$SOURCE_DIR" && pwd -P)" || die "source directory does not exist: $SOURCE_DIR"
mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR="$(cd "$OUTPUT_DIR" && pwd -P)"
validate_source_files
VERSION="$(workspace_version "$SOURCE_DIR")"
ROOT_NAME="videnoa-controller-v${VERSION}-linux-x86_64"
ARCHIVE="$OUTPUT_DIR/${ROOT_NAME}.tar.gz"

if [[ -e "$ARCHIVE" && "$FORCE" != "true" ]]; then
  die "output already exists: $ARCHIVE (use --force to overwrite)"
fi

require_cmd file
if [[ -z "$BINARY_PATH" ]]; then
  require_cmd cargo
  log "building release Controller for $TARGET"
  (cd "$SOURCE_DIR" && cargo build --locked --release -p videnoa-controller --target "$TARGET")
  BINARY_PATH="$SOURCE_DIR/target/$TARGET/release/videnoa-controller"
fi
BINARY_PATH="$(cd "$(dirname "$BINARY_PATH")" && pwd -P)/$(basename "$BINARY_PATH")"
validate_linux_binary "$BINARY_PATH" "$VERSION"

WORK_DIR="$(mktemp -d -t videnoa-controller-package-XXXXXX)"
trap cleanup EXIT
STAGE_ROOT="$WORK_DIR/$ROOT_NAME"
mkdir -p "$STAGE_ROOT"
install -m 0755 "$BINARY_PATH" "$STAGE_ROOT/videnoa-controller"
install -m 0644 "$SOURCE_DIR/controller.example.toml" "$STAGE_ROOT/controller.example.toml"
install -m 0644 "$SOURCE_DIR/README-controller.md" "$STAGE_ROOT/README-controller.md"
install -m 0644 "$SOURCE_DIR/LICENSE" "$STAGE_ROOT/LICENSE"

rm -f "$ARCHIVE"
tar --sort=name --format=ustar --mtime="@$ARCHIVE_EPOCH" --owner=0 --group=0 --numeric-owner \
  --mode='u+rwX,go+rX,go-w' -cf - -C "$WORK_DIR" "$ROOT_NAME" | gzip -n -9 >"$ARCHIVE"
verify_archive "$ARCHIVE"
log "archive created successfully: $ARCHIVE"
