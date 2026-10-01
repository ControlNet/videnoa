#!/usr/bin/env bash
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
readonly SETUP_SCRIPT="$REPO_ROOT/scripts/setup_dev_media_tools.sh"
TEST_TEMP=""

fail() {
  printf '[setup_dev_media_tools_test][error] %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$TEST_TEMP" && -d "$TEST_TEMP" ]]; then
    rm -rf "$TEST_TEMP"
  fi
}

# Build a bundle zip with the release layout (top-level bin/ with SHA256SUMS).
make_bundle() {
  local zip_path="$1"
  local ffmpeg_body="$2"
  local stage="$TEST_TEMP/stage-$RANDOM"
  mkdir -p "$stage/bin"
  printf '#!/bin/sh\necho "%s"\n' "$ffmpeg_body" >"$stage/bin/ffmpeg"
  printf '#!/bin/sh\necho ffprobe\n' >"$stage/bin/ffprobe"
  printf '#!/bin/sh\necho mkvpropedit\n' >"$stage/bin/mkvpropedit"
  chmod +x "$stage/bin/ffmpeg" "$stage/bin/ffprobe" "$stage/bin/mkvpropedit"
  (cd "$stage/bin" && sha256sum ffmpeg ffprobe mkvpropedit >SHA256SUMS)
  (cd "$stage" && zip -qr "$zip_path" bin)
}

TEST_TEMP="$(mktemp -d -t videnoa-setup-media-test-XXXXXX)"
trap cleanup EXIT
target_root="$TEST_TEMP/repo"
mkdir -p "$target_root"

make_bundle "$TEST_TEMP/good.zip" "ffmpeg version n8.1-test"

"$SETUP_SCRIPT" --zip "$TEST_TEMP/good.zip" --dest "$target_root/bin" >/dev/null \
  || fail "installing a valid bundle failed"
[[ "$("$target_root/bin/ffmpeg")" == "ffmpeg version n8.1-test" ]] \
  || fail "bin/ffmpeg was not installed from the bundle"
[[ -x "$target_root/bin/ffprobe" && -x "$target_root/bin/mkvpropedit" ]] \
  || fail "ffprobe/mkvpropedit were not installed"

# An existing bin/ is kept unless --force is given.
make_bundle "$TEST_TEMP/newer.zip" "ffmpeg version n9-test"
if "$SETUP_SCRIPT" --zip "$TEST_TEMP/newer.zip" --dest "$target_root/bin" >/dev/null 2>&1; then
  fail "an existing bin/ was replaced without --force"
fi
[[ "$("$target_root/bin/ffmpeg")" == "ffmpeg version n8.1-test" ]] \
  || fail "a refused install modified the existing bin/"

"$SETUP_SCRIPT" --zip "$TEST_TEMP/newer.zip" --dest "$target_root/bin" --force >/dev/null \
  || fail "--force did not replace the existing bin/"
[[ "$("$target_root/bin/ffmpeg")" == "ffmpeg version n9-test" ]] \
  || fail "--force left the old ffmpeg in place"

# A bundle whose checksums do not match must not be installed.
stage="$TEST_TEMP/corrupt"
mkdir -p "$stage/bin"
printf '#!/bin/sh\necho corrupt\n' >"$stage/bin/ffmpeg"
chmod +x "$stage/bin/ffmpeg"
printf '%064d  ffmpeg\n' 0 >"$stage/bin/SHA256SUMS"
(cd "$stage" && zip -qr "$TEST_TEMP/corrupt.zip" bin)
if "$SETUP_SCRIPT" --zip "$TEST_TEMP/corrupt.zip" --dest "$TEST_TEMP/corrupt-dest" >/dev/null 2>&1; then
  fail "a bundle with mismatching checksums was installed"
fi
[[ ! -e "$TEST_TEMP/corrupt-dest" ]] \
  || fail "a failed install left a partial destination"

printf '[setup_dev_media_tools_test] ok\n'
