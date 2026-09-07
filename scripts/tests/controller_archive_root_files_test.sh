#!/usr/bin/env bash
set -euo pipefail

readonly REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"

fail() {
  printf '[controller_archive_root_files_test][error] %s\n' "$*" >&2
  exit 1
}

for name in controller.example.toml README-controller.md LICENSE; do
  [[ -s "$REPO_ROOT/$name" ]] || fail "missing or empty archive root file: $name"
done

if grep -Eiq '^\s*(password|password_hash|admin_password)\s*=' "$REPO_ROOT/controller.example.toml"; then
  fail "example configuration contains credential material"
fi
for section in server auth scheduler timeouts retry; do
  grep -Fq "[$section]" "$REPO_ROOT/controller.example.toml" \
    || fail "example configuration is missing [$section]"
done
if grep -Eq '\[paths\]|password_hash_file|hash-password|admin-password\.phc' "$REPO_ROOT/controller.example.toml" "$REPO_ROOT/README-controller.md"; then
  fail "archive root files still require prepared paths or password hashes"
fi
grep -Fq './videnoa-controller' "$REPO_ROOT/README-controller.md" \
  || fail "Controller README is missing zero-config startup"
grep -Fq '/api/auth/setup' "$REPO_ROOT/README-controller.md" \
  || fail "Controller README is missing first-administrator setup"
grep -Fq '/api/health' "$REPO_ROOT/README-controller.md" \
  || fail "Controller README is missing a health smoke command"

printf '[controller_archive_root_files_test] root files are complete and secret-free\n'
