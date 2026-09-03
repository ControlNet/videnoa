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

grep -Fq 'password_hash_file = ' "$REPO_ROOT/controller.example.toml" \
  || fail "example configuration must reference a password hash file"
if grep -Eiq '^\s*(password|password_hash|admin_password)\s*=' "$REPO_ROOT/controller.example.toml"; then
  fail "example configuration contains credential material"
fi
for section in server paths auth scheduler timeouts retry; do
  grep -Fq "[$section]" "$REPO_ROOT/controller.example.toml" \
    || fail "example configuration is missing [$section]"
done
grep -Fq 'hash-password' "$REPO_ROOT/README-controller.md" \
  || fail "Controller README is missing password hash setup"
grep -Fq -- '--config' "$REPO_ROOT/README-controller.md" \
  || fail "Controller README is missing explicit configuration startup"
grep -Fq '/api/health' "$REPO_ROOT/README-controller.md" \
  || fail "Controller README is missing a health smoke command"

printf '[controller_archive_root_files_test] root files are complete and secret-free\n'
