#!/usr/bin/env bash
# Install the release media tools bundle (FFmpeg, ffprobe, mkvpropedit) into the
# checkout's bin/ so development runs use the same tools as release packages.
# runtime::command_for searches <cwd>/bin before PATH, so run Videnoa from the
# repository root after installing.
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
readonly DEFAULT_URL="https://github.com/ControlNet/videnoa/releases/download/misc/bin_linux64.zip"

DEST="$REPO_ROOT/bin"
ZIP_PATH=""
URL="$DEFAULT_URL"
FORCE="false"
WORK_DIR=""

usage() {
  cat <<'EOF'
Install the Linux release media tools bundle into <repo>/bin.

Usage:
  scripts/setup_dev_media_tools.sh [options]

Options:
  --zip <path>    Use a local bin_linux64.zip instead of downloading
  --url <url>     Download URL (default: misc release bin_linux64.zip)
  --dest <dir>    Install directory (default: <repo>/bin)
  --force         Replace an existing install directory
  -h, --help      Show this help message
EOF
}

log() {
  printf '[setup_dev_media_tools] %s\n' "$*"
}

die() {
  printf '[setup_dev_media_tools][error] %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]]; then
    rm -rf "$WORK_DIR"
  fi
}

while (($# > 0)); do
  case "$1" in
    --zip)
      ZIP_PATH="${2:?--zip requires a path}"
      shift 2
      ;;
    --url)
      URL="${2:?--url requires a value}"
      shift 2
      ;;
    --dest)
      DEST="${2:?--dest requires a directory}"
      shift 2
      ;;
    --force)
      FORCE="true"
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

if [[ -e "$DEST" && "$FORCE" != "true" ]]; then
  die "$DEST already exists (use --force to replace it)"
fi

command -v unzip >/dev/null || die "unzip is required"
command -v sha256sum >/dev/null || die "sha256sum is required"

trap cleanup EXIT
WORK_DIR="$(mktemp -d -t videnoa-media-tools-XXXXXX)"

if [[ -z "$ZIP_PATH" ]]; then
  command -v wget >/dev/null || die "wget is required to download $URL"
  ZIP_PATH="$WORK_DIR/bin_linux64.zip"
  log "downloading $URL"
  wget -q --tries=4 --timeout=120 -O "$ZIP_PATH" "$URL" || die "download failed: $URL"
fi
[[ -s "$ZIP_PATH" ]] || die "bundle is missing or empty: $ZIP_PATH"

unzip -q "$ZIP_PATH" -d "$WORK_DIR/extract"
staged="$WORK_DIR/extract/bin"
[[ -d "$staged" ]] || die "bundle has no top-level bin/ directory"
[[ -x "$staged/ffmpeg" ]] || die "bundle has no executable bin/ffmpeg"

if [[ -f "$staged/SHA256SUMS" ]]; then
  (cd "$staged" && sha256sum --quiet -c SHA256SUMS) || die "bundle checksum verification failed"
else
  die "bundle has no bin/SHA256SUMS"
fi

if [[ -e "$DEST" ]]; then
  log "replacing $DEST"
  rm -rf "$DEST"
fi
mkdir -p "$(dirname "$DEST")"
mv "$staged" "$DEST"

log "installed into $DEST"
"$DEST/ffmpeg" -hide_banner -version 2>/dev/null | head -n 1 || true
