#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Create or verify the existing Videnoa distribution archive.

Usage:
  scripts/package_dist_archive.sh create <dist-root> <archive-path> [volume-size]
  scripts/package_dist_archive.sh verify <archive-path>
EOF
}

die() {
  printf '[package-dist-archive][error] %s\n' "$*" >&2
  exit 1
}

require_7z() {
  command -v 7z >/dev/null 2>&1 || die "7z is required"
}

create_archive() {
  [[ $# -eq 2 || $# -eq 3 ]] || die "create requires <dist-root> <archive-path> [volume-size]"
  local dist_root="$1"
  local archive_path="$2"
  local volume_size="${3:-2000m}"
  local bundle_dir="$dist_root/videnoa"
  local archive_dir
  local bundle_kb
  local available_kb
  local required_kb
  local disk_report

  [[ -d "$bundle_dir" ]] || die "Expected bundle directory not found: $bundle_dir"
  [[ "$archive_path" = /* ]] || archive_path="$(pwd -P)/$archive_path"
  archive_dir="$(dirname "$archive_path")"
  [[ -d "$archive_dir" ]] || die "Archive output directory not found: $archive_dir"

  bundle_kb="$(du -sk "$bundle_dir" | cut -f1)"
  disk_report="$(df -Pk "$archive_dir")"
  available_kb="$(awk 'END {print $4}' <<<"$disk_report")"
  required_kb=$((bundle_kb + 65536))
  if ((available_kb < required_kb)); then
    die "Insufficient archive disk space: need ${required_kb} KiB, available ${available_kb} KiB in $archive_dir"
  fi
  printf '[package-dist-archive] bundle=%s KiB required=%s KiB available=%s KiB\n' "$bundle_kb" "$required_kb" "$available_kb"

  rm -f "$archive_path" "$archive_path".*
  (
    cd "$dist_root"
    7z a -t7z "-v${volume_size}" "$archive_path" videnoa
  )
  if [[ ! -f "$archive_path.001" && ! -f "$archive_path" ]]; then
    die "Missing archive output: $archive_path(.001)"
  fi
}

verify_archive() {
  [[ $# -eq 1 ]] || die "verify requires <archive-path>"
  local archive_base="$1"
  local archive_input="$archive_base.001"

  if [[ ! -f "$archive_input" ]]; then
    if [[ -f "$archive_base" ]]; then
      archive_input="$archive_base"
    else
      die "Missing archive output: $archive_base(.001)"
    fi
  fi

  7z t "$archive_input" >/dev/null
  7z l "$archive_input" | grep -E 'videnoa[/\\]' >/dev/null || die "Archive root layout validation failed: missing videnoa/ root"
}

main() {
  [[ $# -ge 1 ]] || {
    usage >&2
    exit 1
  }
  require_7z

  local command="$1"
  shift
  case "$command" in
    create)
      create_archive "$@"
      ;;
    verify)
      verify_archive "$@"
      ;;
    -h|--help)
      usage
      ;;
    *)
      die "unknown command: $command"
      ;;
  esac
}

main "$@"
