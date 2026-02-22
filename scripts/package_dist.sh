#!/usr/bin/env bash
set -euo pipefail

DEFAULT_REPO="ControlNet/videnoa"
DEFAULT_RELEASE_TAG="misc"

REPO="$DEFAULT_REPO"
RELEASE_TAG="$DEFAULT_RELEASE_TAG"
PLATFORM="auto"
OUTPUT_DIR="$PWD"
WORK_DIR=""
SOURCE_DIR=""
KEEP_WORK_DIR="false"
FORCE_OVERWRITE="false"
WORK_DIR_EPHEMERAL="false"

usage() {
  cat <<'EOF'
Package Videnoa distribution folder.

This script will:
1) clone ControlNet/videnoa
2) run cargo build --release --workspace
3) download platform assets from GitHub release (lib/bin/models)
4) assemble a distribution folder named "videnoa"

Usage:
  scripts/package_dist.sh [options]

Options:
  --repo <owner/name>       GitHub repository (default: ControlNet/videnoa)
  --release-tag <tag>       Release tag for large assets (default: misc)
  --platform <auto|linux64|win64>
                            Asset platform selector (default: auto)
  --output-dir <path>       Parent directory for output folder "videnoa" (default: current directory)
  --work-dir <path>         Working directory (default: temporary directory)
  --source-dir <path>       Use a local source checkout instead of cloning from GitHub
  --keep-work-dir           Keep work directory after completion
  --force                   Remove existing output "videnoa" folder if present
  -h, --help                Show this help message

Examples:
  scripts/package_dist.sh
  scripts/package_dist.sh --output-dir ./dist --force
  scripts/package_dist.sh --platform linux64 --release-tag misc
  scripts/package_dist.sh --source-dir /path/to/local/videnoa --force
EOF
}

log() {
  printf '[package_dist] %s\n' "$*"
}

warn() {
  printf '[package_dist][warn] %s\n' "$*" >&2
}

die() {
  printf '[package_dist][error] %s\n' "$*" >&2
  exit 1
}

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    die "required command not found: $cmd"
  fi
}

build_frontend_assets() {
  local repo_root="$1"
  local web_dir="$repo_root/web"

  [[ -d "$web_dir" ]] || die "missing frontend directory: $web_dir"

  local install_cmd
  if [[ -f "$web_dir/package-lock.json" ]]; then
    install_cmd="ci"
  else
    install_cmd="install"
  fi

  log "installing frontend dependencies (npm ${install_cmd} --no-fund)"
  (
    cd "$web_dir"
    npm "$install_cmd" --no-fund
  )

  log "building frontend assets (npm run build)"
  (
    cd "$web_dir"
    npm run build
  )

  [[ -d "$web_dir/dist" ]] || die "frontend build did not produce dist directory: $web_dir/dist"
}

download_release_asset() {
  local asset_name="$1"
  local output_file="$2"
  local release_url
  release_url="https://github.com/${REPO}/releases/download/${RELEASE_TAG}/${asset_name}"

  log "downloading asset: ${asset_name}"
  if ! wget -q -O "$output_file" "$release_url"; then
    die "failed to download asset '${asset_name}' from ${release_url}"
  fi
}

validate_source_tree() {
  local repo_root="$1"
  local required=(
    "Cargo.toml"
    "crates/app/Cargo.toml"
    "web/package.json"
    "web/src/lib/utils.ts"
    "web/src/lib/runtime-desktop.ts"
    "web/src/lib/presentation-error.ts"
    "web/src/lib/presentation-format.ts"
  )

  local missing=()
  local path
  for path in "${required[@]}"; do
    if [[ ! -f "$repo_root/$path" ]]; then
      missing+=("$path")
    fi
  done

  if [[ ${#missing[@]} -gt 0 ]]; then
    printf '[package_dist][error] source tree is missing required files:\n' >&2
    printf '  - %s\n' "${missing[@]}" >&2
    printf '[package_dist][error] this usually means the selected ref is missing recently added frontend files (or they were ignored and never committed).\n' >&2
    printf '[package_dist][error] fix by packaging from a local source checkout with --source-dir, or commit/push missing files first.\n' >&2
    exit 1
  fi
}

detect_platform() {
  local uname_s
  uname_s="$(uname -s)"
  case "$uname_s" in
    Linux*)
      printf 'linux64\n'
      ;;
    MINGW*|MSYS*|CYGWIN*)
      printf 'win64\n'
      ;;
    *)
      die "unsupported host platform '$uname_s'. Use --platform explicitly (linux64 or win64)."
      ;;
  esac
}

cleanup() {
  if [[ -z "${WORK_DIR:-}" ]]; then
    return
  fi

  if [[ "$WORK_DIR_EPHEMERAL" != "true" ]]; then
    if [[ "$KEEP_WORK_DIR" == "true" ]]; then
      log "keeping user-provided work directory: $WORK_DIR"
    fi
    return
  fi

  if [[ "$KEEP_WORK_DIR" == "true" ]]; then
    log "keeping work directory: $WORK_DIR"
    return
  fi

  if [[ -d "$WORK_DIR" ]]; then
    rm -rf "$WORK_DIR"
  fi
}

extract_zip_into_dir() {
  local zip_file="$1"
  local expected_root="$2"
  local dest_dir="$3"

  local temp_extract
  temp_extract="$(mktemp -d -t videnoa-unzip-XXXXXX)"

  unzip -q "$zip_file" -d "$temp_extract"
  mkdir -p "$dest_dir"

  if [[ -d "$temp_extract/$expected_root" ]]; then
    cp -a "$temp_extract/$expected_root"/. "$dest_dir"/
    rm -rf "$temp_extract"
    return
  fi

  shopt -s nullglob dotglob
  local entries=("$temp_extract"/*)
  shopt -u nullglob dotglob

  if [[ ${#entries[@]} -eq 1 && -d "${entries[0]}" ]]; then
    cp -a "${entries[0]}"/. "$dest_dir"/
  else
    cp -a "$temp_extract"/. "$dest_dir"/
  fi

  rm -rf "$temp_extract"
}

validate_bundle_layout() {
  local bundle_dir="$1"
  local expected=("videnoa" "videnoa-desktop" "lib" "bin" "models" "presets" "README.md" "LICENSE")

  local ok="true"

  for name in "${expected[@]}"; do
    if [[ ! -e "$bundle_dir/$name" ]]; then
      warn "missing required entry: $name"
      ok="false"
    fi
  done

  shopt -s nullglob
  local entries=("$bundle_dir"/*)
  shopt -u nullglob

  for path in "${entries[@]}"; do
    local base
    base="$(basename "$path")"
    local found="false"
    for name in "${expected[@]}"; do
      if [[ "$base" == "$name" ]]; then
        found="true"
        break
      fi
    done

    if [[ "$found" == "false" ]]; then
      warn "unexpected extra entry: $base"
      ok="false"
    fi
  done

  if [[ ! -d "$bundle_dir/lib" || ! -d "$bundle_dir/bin" || ! -d "$bundle_dir/models" || ! -d "$bundle_dir/presets" ]]; then
    warn "one or more required directories are invalid"
    ok="false"
  fi

  if [[ "$ok" != "true" ]]; then
    die "bundle layout validation failed"
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      [[ $# -ge 2 ]] || die "missing value for --repo"
      REPO="$2"
      shift 2
      ;;
    --release-tag)
      [[ $# -ge 2 ]] || die "missing value for --release-tag"
      RELEASE_TAG="$2"
      shift 2
      ;;
    --platform)
      [[ $# -ge 2 ]] || die "missing value for --platform"
      PLATFORM="$2"
      shift 2
      ;;
    --output-dir)
      [[ $# -ge 2 ]] || die "missing value for --output-dir"
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --work-dir)
      [[ $# -ge 2 ]] || die "missing value for --work-dir"
      WORK_DIR="$2"
      WORK_DIR_EPHEMERAL="false"
      shift 2
      ;;
    --source-dir)
      [[ $# -ge 2 ]] || die "missing value for --source-dir"
      SOURCE_DIR="$2"
      shift 2
      ;;
    --keep-work-dir)
      KEEP_WORK_DIR="true"
      shift
      ;;
    --force)
      FORCE_OVERWRITE="true"
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

trap cleanup EXIT

require_cmd git
require_cmd cargo
require_cmd npm
require_cmd wget
require_cmd unzip
require_cmd mktemp

if [[ "$PLATFORM" == "auto" ]]; then
  PLATFORM="$(detect_platform)"
fi

case "$PLATFORM" in
  linux64)
    BIN_ASSET="bin_linux64.zip"
    LIB_PART_1="lib_linux64.zip.001"
    LIB_PART_2="lib_linux64.zip.002"
    EXE_SUFFIX=""
    ;;
  win64)
    BIN_ASSET="bin_win64.zip"
    LIB_PART_1="lib_win64.zip.001"
    LIB_PART_2="lib_win64.zip.002"
    EXE_SUFFIX=".exe"
    ;;
  *)
    die "unsupported platform: $PLATFORM"
    ;;
esac

mkdir -p "$OUTPUT_DIR"

if [[ -z "$WORK_DIR" ]]; then
  WORK_DIR="$(mktemp -d -t videnoa-pack-XXXXXX)"
  WORK_DIR_EPHEMERAL="true"
else
  mkdir -p "$WORK_DIR"
fi

CLONE_DIR="$WORK_DIR/repo"
DOWNLOAD_DIR="$WORK_DIR/download"
BUNDLE_DIR="$OUTPUT_DIR/videnoa"

if [[ -e "$BUNDLE_DIR" ]]; then
  if [[ "$FORCE_OVERWRITE" == "true" ]]; then
    warn "removing existing bundle directory: $BUNDLE_DIR"
    rm -rf "$BUNDLE_DIR"
  else
    die "output already exists: $BUNDLE_DIR (use --force to overwrite)"
  fi
fi

mkdir -p "$DOWNLOAD_DIR"
rm -rf "$CLONE_DIR"

if [[ -n "$SOURCE_DIR" ]]; then
  [[ -d "$SOURCE_DIR" ]] || die "--source-dir is not a directory: $SOURCE_DIR"
  local_source="$(cd "$SOURCE_DIR" && pwd -P)"
  [[ -d "$local_source" ]] || die "--source-dir is not a directory: $SOURCE_DIR"
  [[ -f "$local_source/Cargo.toml" ]] || die "--source-dir does not look like videnoa repository root: $SOURCE_DIR"

  log "copying source tree from local checkout: $local_source"
  mkdir -p "$CLONE_DIR"
  cp -a "$local_source"/. "$CLONE_DIR"/
else
  log "cloning repository: https://github.com/${REPO}.git"
  git clone --depth 1 "https://github.com/${REPO}.git" "$CLONE_DIR"
fi

validate_source_tree "$CLONE_DIR"

build_frontend_assets "$CLONE_DIR"

log "building release workspace"
(
  cd "$CLONE_DIR"
  cargo build --release --workspace
)

log "downloading release assets from tag '$RELEASE_TAG'"
download_release_asset "$BIN_ASSET" "$DOWNLOAD_DIR/$BIN_ASSET"
download_release_asset "$LIB_PART_1" "$DOWNLOAD_DIR/$LIB_PART_1"
download_release_asset "$LIB_PART_2" "$DOWNLOAD_DIR/$LIB_PART_2"
download_release_asset "models.zip" "$DOWNLOAD_DIR/models.zip"

log "merging split lib archive"
MERGED_LIB_ZIP="$DOWNLOAD_DIR/lib_${PLATFORM}.zip"
cat "$DOWNLOAD_DIR/$LIB_PART_1" "$DOWNLOAD_DIR/$LIB_PART_2" > "$MERGED_LIB_ZIP"

log "assembling bundle directory: $BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR"

VIDENOA_BIN_SRC="$CLONE_DIR/target/release/videnoa${EXE_SUFFIX}"
VIDENOA_DESKTOP_BIN_SRC="$CLONE_DIR/target/release/videnoa-desktop${EXE_SUFFIX}"

if [[ ! -f "$VIDENOA_BIN_SRC" ]]; then
  die "missing build output: $VIDENOA_BIN_SRC"
fi
if [[ ! -f "$VIDENOA_DESKTOP_BIN_SRC" ]]; then
  die "missing build output: $VIDENOA_DESKTOP_BIN_SRC"
fi

cp "$VIDENOA_BIN_SRC" "$BUNDLE_DIR/videnoa"
cp "$VIDENOA_DESKTOP_BIN_SRC" "$BUNDLE_DIR/videnoa-desktop"

if [[ "$PLATFORM" == "linux64" ]]; then
  chmod +x "$BUNDLE_DIR/videnoa" "$BUNDLE_DIR/videnoa-desktop"
fi

extract_zip_into_dir "$MERGED_LIB_ZIP" "lib" "$BUNDLE_DIR/lib"
extract_zip_into_dir "$DOWNLOAD_DIR/$BIN_ASSET" "bin" "$BUNDLE_DIR/bin"
extract_zip_into_dir "$DOWNLOAD_DIR/models.zip" "models" "$BUNDLE_DIR/models"

cp -a "$CLONE_DIR/presets" "$BUNDLE_DIR/presets"
cp "$CLONE_DIR/README.md" "$BUNDLE_DIR/README.md"
cp "$CLONE_DIR/LICENSE" "$BUNDLE_DIR/LICENSE"

validate_bundle_layout "$BUNDLE_DIR"

log "bundle created successfully: $BUNDLE_DIR"
