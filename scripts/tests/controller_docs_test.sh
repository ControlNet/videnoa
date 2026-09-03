#!/usr/bin/env bash
set -euo pipefail

readonly REPO_ROOT="${CONTROLLER_DOCS_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)}"
readonly GUIDE="$REPO_ROOT/docs/controller.md"
readonly ARCHIVE_GUIDE="$REPO_ROOT/README-controller.md"
readonly ROOT_README="$REPO_ROOT/README.md"
readonly CONFIG="$REPO_ROOT/controller.example.toml"

fail() {
  printf '[controller-docs][error] %s\n' "$*" >&2
  exit 1
}

require_text() {
  local file="$1"
  local text="$2"
  grep -Fq -- "$text" "$file" || fail "$(basename "$file") is missing: $text"
}

for file in "$GUIDE" "$ARCHIVE_GUIDE" "$ROOT_README" "$CONFIG"; do
  [[ -f "$file" ]] || fail "missing required file: ${file#"$REPO_ROOT/"}"
done

for text in \
  '## Architecture and Boundaries' \
  '## Security' \
  '## API Reference' \
  '## Backup and Restore' \
  '## Upgrade and Rollback' \
  '## Troubleshooting' \
  'SQLite is authoritative' \
  'jobs.db' \
  'remote_state_ambiguous' \
  'publication_ambiguous' \
  'controlnet/videnoa-controller:<version>' \
  'videnoa-controller-v<version>-linux-x86_64.tar.gz' \
  'videnoa-controller-v<version>-windows-x86_64.zip'; do
  require_text "$GUIDE" "$text"
done

for text in \
  '## First Run' \
  '## Critical Recovery' \
  'controller.sqlite3' \
  'jobs.db' \
  'remote_state_ambiguous' \
  'publication_ambiguous'; do
  require_text "$ARCHIVE_GUIDE" "$text"
done

for route in \
  '/api/health' \
  '/api/readiness' \
  '/api/auth/login' \
  '/api/auth/session' \
  '/api/auth/logout' \
  '/api/tasks' \
  '/api/tasks/{id}' \
  '/api/tasks/{id}/cancel' \
  '/api/tasks/{id}/retry' \
  '/api/status-counts' \
  '/api/workers' \
  '/api/settings' \
  '/api/scheduler/pause' \
  '/api/scheduler/resume' \
  '/api/events'; do
  require_text "$GUIDE" "$route"
done

for field in input_path output_path workflow priority source source_reference; do
  require_text "$GUIDE" "$field"
done

for section in server paths auth scheduler timeouts retry; do
  require_text "$CONFIG" "[$section]"
done

if grep -Eiq 'videnoa-worker|videnoa-controller-linux-|videnoa-controller-windows-|controlnet/videnoa-controller:(stable|version)' "$GUIDE" "$ARCHIVE_GUIDE" "$ROOT_README"; then
  fail 'documentation contains a stale or forbidden product/artifact name'
fi

if grep -Ein '^[[:space:]]*(password|token|secret|cookie|csrf)[[:space:]]*=[[:space:]]*"[^$%{<]' "$CONFIG" "$GUIDE" "$ARCHIVE_GUIDE"; then
  fail 'documentation contains a plaintext secret assignment'
fi

while IFS= read -r link; do
  target="${link%%#*}"
  [[ -z "$target" || "$target" == http://* || "$target" == https://* || "$target" == mailto:* ]] && continue
  if [[ "$target" == ../* ]]; then
    target="${target#../}"
  fi
  [[ -e "$REPO_ROOT/$target" ]] || fail "broken repository link: $link"
done < <(grep -Eho '\]\(([^)]+)\)' "$GUIDE" "$ARCHIVE_GUIDE" "$ROOT_README" | sed -E 's/^\]\((.*)\)$/\1/')

printf '[controller-docs] required topics, routes, fields, names, links, and secret patterns: PASS\n'
