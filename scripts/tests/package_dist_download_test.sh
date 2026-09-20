#!/usr/bin/env bash
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
readonly PACKAGE_SCRIPT="$REPO_ROOT/scripts/package_dist.sh"
TEST_TEMP=""
DOWNLOAD_ATTEMPTS=0
LAST_WGET_ARGS=""

fail() {
  printf '[package_dist_download_test][error] %s\n' "$*" >&2
  exit 1
}

cleanup() {
  if [[ -n "$TEST_TEMP" && -d "$TEST_TEMP" ]]; then
    rm -rf "$TEST_TEMP"
  fi
}

# Load only the production helpers; do not execute the packaging entry point.
source <(awk '/^validate_source_tree\(\)/ { exit } { print }' "$PACKAGE_SCRIPT")

# Deterministic test-only command double: fail twice, then complete the asset.
wget() {
  DOWNLOAD_ATTEMPTS=$((DOWNLOAD_ATTEMPTS + 1))
  LAST_WGET_ARGS="$*"
  local output_file=""

  while (($# > 0)); do
    if [[ "$1" == "-O" ]]; then
      output_file="$2"
      break
    fi
    shift
  done

  [[ -n "$output_file" ]] || fail "wget was not given an output file"
  if ((DOWNLOAD_ATTEMPTS < 3)); then
    printf 'partial-%s\n' "$DOWNLOAD_ATTEMPTS" >>"$output_file"
    return 1
  fi

  printf 'complete\n' >"$output_file"
}

sleep() {
  :
}

TEST_TEMP="$(mktemp -d -t videnoa-package-download-test-XXXXXX)"
trap cleanup EXIT
output_file="$TEST_TEMP/asset.zip.001"

download_release_asset "asset.zip.001" "$output_file"

[[ "$DOWNLOAD_ATTEMPTS" == "3" ]] \
  || fail "expected three download attempts, got $DOWNLOAD_ATTEMPTS"
[[ "$(<"$output_file")" == "complete" ]] \
  || fail "successful retry did not replace the partial fixture"
for option in \
  '--continue' \
  '--tries=8' \
  '--retry-connrefused' \
  '--retry-on-http-error=429,500,502,503,504' \
  '--timeout=120' \
  '--read-timeout=120'; do
  [[ " $LAST_WGET_ARGS " == *" $option "* ]] \
    || fail "wget retry contract is missing $option"
done

printf '[package_dist_download_test] retry and resume contract: PASS\n'
