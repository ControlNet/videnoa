#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
archive_helper="$repo_root/scripts/package_dist_archive.sh"
test_root="$(mktemp -d -t videnoa-archive-test-XXXXXX)"
trap 'rm -rf "$test_root"' EXIT

fail() {
  printf '[package-dist-archive-test][error] %s\n' "$*" >&2
  exit 1
}

require_file() {
  [[ -f "$1" ]] || fail "missing expected file: $1"
}

command -v 7z >/dev/null 2>&1 || fail "7z is required"
[[ -x "$archive_helper" ]] || fail "archive helper is missing or not executable: $archive_helper"

mkdir -p "$test_root/dist/videnoa/nested" "$test_root/out"
printf 'archive fixture\n' > "$test_root/dist/videnoa/nested/payload.txt"

split_archive="$test_root/out/videnoa-linux64-smoke.7z"
"$archive_helper" create "$test_root/dist" "$split_archive" 1k
require_file "$split_archive.001"
"$archive_helper" verify "$split_archive"

single_archive="$test_root/out/videnoa-linux64-single.7z"
(
  cd "$test_root/dist"
  7z a -t7z "$single_archive" videnoa >/dev/null
)
require_file "$single_archive"
"$archive_helper" verify "$single_archive"

mkdir -p "$test_root/command-bin"
cat > "$test_root/command-bin/7z" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$PACKAGE_ARCHIVE_7Z_ARGS"
args=("$@")
archive_index=$((${#args[@]} - 2))
touch "${args[$archive_index]}.001"
EOF
chmod +x "$test_root/command-bin/7z"

resource_archive="$test_root/out/resource-safe.7z"
resource_args="$test_root/resource-safe.args"
PACKAGE_ARCHIVE_7Z_ARGS="$resource_args" PATH="$test_root/command-bin:$PATH" \
  "$archive_helper" create "$test_root/dist" "$resource_archive"
expected_resource_args=(
  a
  -t7z
  -mx=5
  -md=16m
  -mmt=1
  -v2000m
  "$resource_archive"
  videnoa
)
mapfile -t actual_resource_args < "$resource_args"
[[ "${actual_resource_args[*]}" == "${expected_resource_args[*]}" ]] ||
  fail "archive creation did not use the exact bounded p7zip settings"

if "$archive_helper" verify "$test_root/out/missing.7z" >"$test_root/missing.stdout" 2>"$test_root/missing.stderr"; then
  fail "missing archive verification unexpectedly succeeded"
fi
grep -Fq 'Missing archive output:' "$test_root/missing.stderr" || fail "missing archive failure was not explicit"

mkdir -p "$test_root/fake-bin"
cat > "$test_root/fake-bin/7z" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$test_root/fake-bin/7z"

if PATH="$test_root/fake-bin:$PATH" "$archive_helper" create "$test_root/dist" "$test_root/out/missing-create.7z" 1k >"$test_root/missing-create.stdout" 2>"$test_root/missing-create.stderr"; then
  fail "archive creation without output unexpectedly succeeded"
fi
grep -Fq 'Missing archive output:' "$test_root/missing-create.stderr" || fail "missing creation output failure was not explicit"

cat > "$test_root/fake-bin/df" <<'EOF'
#!/usr/bin/env bash
printf 'Filesystem 1024-blocks Used Available Capacity Mounted on\n'
printf '/dev/fake 1024 1023 1 100%% /tmp\n'
EOF
chmod +x "$test_root/fake-bin/df"

cat > "$test_root/fake-bin/7z" <<'EOF'
#!/usr/bin/env bash
printf '7z must not run after an insufficient-space preflight\n' >&2
exit 99
EOF
chmod +x "$test_root/fake-bin/7z"

if PATH="$test_root/fake-bin:$PATH" "$archive_helper" create "$test_root/dist" "$test_root/out/no-space.7z" 1k >"$test_root/no-space.stdout" 2>"$test_root/no-space.stderr"; then
  fail "insufficient archive disk space unexpectedly succeeded"
fi
grep -Fq 'Insufficient archive disk space:' "$test_root/no-space.stderr" || fail "insufficient-space failure was not explicit"
rm "$test_root/fake-bin/df"

cat > "$test_root/fake-bin/7z" <<'EOF'
#!/usr/bin/env bash
printf 'System ERROR:\nE_FAIL\n' >&2
exit 2
EOF
chmod +x "$test_root/fake-bin/7z"

fatal_status=0
PATH="$test_root/fake-bin:$PATH" "$archive_helper" create "$test_root/dist" "$test_root/out/fatal.7z" 1k >"$test_root/fatal.stdout" 2>"$test_root/fatal.stderr" || fatal_status=$?
[[ "$fatal_status" -eq 2 ]] || fail "7z fatal exit 2 changed to status $fatal_status"
grep -Fq 'E_FAIL' "$test_root/fatal.stderr" || fail "7z fatal diagnostics were not preserved"

printf '[package-dist-archive-test] split, single, resource-safe command, missing-output, missing-create-output, insufficient-space, and fatal-create contracts passed\n'
