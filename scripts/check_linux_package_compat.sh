#!/usr/bin/env bash
set -euo pipefail

readonly MAX_GLIBC_MAJOR=2
readonly MAX_GLIBC_MINOR=35

die() {
  printf '[linux-compat][error] %s\n' "$*" >&2
  exit 1
}

check_glibc_requirements() {
  local binary="$1"
  local version major minor
  local incompatible=()

  while IFS= read -r version; do
    [[ -n "$version" ]] || continue
    major="${version%%.*}"
    minor="${version#*.}"
    if (( major > MAX_GLIBC_MAJOR || (major == MAX_GLIBC_MAJOR && minor > MAX_GLIBC_MINOR) )); then
      incompatible+=("GLIBC_${version}")
    fi
  done < <(
    objdump -T "$binary" \
      | grep -oE 'GLIBC_[0-9]+\.[0-9]+' \
      | cut -d_ -f2 \
      | sort -Vu
  )

  if (( ${#incompatible[@]} > 0 )); then
    printf '[linux-compat][error] %s requires unsupported glibc symbols:\n' "$binary" >&2
    printf '  - %s\n' "${incompatible[@]}" >&2
    die "Linux packages must remain compatible with glibc ${MAX_GLIBC_MAJOR}.${MAX_GLIBC_MINOR}"
  fi
}

if (( $# != 1 )); then
  die "usage: $0 <linux-bundle-dir>"
fi

readonly BUNDLE_DIR="$1"
readonly CLI="$BUNDLE_DIR/videnoa"
readonly DESKTOP="$BUNDLE_DIR/videnoa-desktop"

command -v objdump >/dev/null 2>&1 || die "objdump is required"
[[ -x "$CLI" ]] || die "missing executable: $CLI"
[[ -x "$DESKTOP" ]] || die "missing executable: $DESKTOP"

"$CLI" --help >/dev/null
check_glibc_requirements "$CLI"
check_glibc_requirements "$DESKTOP"

printf '[linux-compat] package starts and requires glibc <= %d.%d\n' \
  "$MAX_GLIBC_MAJOR" "$MAX_GLIBC_MINOR"
