# CI Node.js 24 migration (2026-09-08)

Local Node.js is `v24.11.1`, provided by nvm at
`/home/zhixi/.nvm/versions/node/v24.11.1/bin/node`. Both Dockerfiles already
build frontend assets with `node:24-bookworm-slim`.

GitHub's Node 20 Actions deprecation concerns each JavaScript action's own
`runs.using` runtime, separately from the Node version installed by setup-node.
Updating only `node-version` does not update checkout/artifact/etc. runtimes.
Source: https://github.blog/changelog/2025-09-19-deprecation-of-node-20-on-github-actions-runners/

Both unittest and release workflows now request Node 24. The selected action
major versions are the Node 24 migrations, avoiding unrelated newer majors:

- checkout v5; setup-node v5
- upload-artifact v6; download-artifact v7
- Docker setup-buildx v4, login v4, build-push v7
- softprops/action-gh-release v3

Every referenced JavaScript action's current `action.yml` was fetched and
confirmed to declare node24. Swatinem/rust-cache v2 already does; the pinned
Rust toolchain action is composite. GitHub-hosted runners meet the required
runner version (Node 24 action releases require at least 2.327.1).

arduino/setup-protoc v3 still declares node20 and had no newer release. Replaced
all three Windows uses with `scripts/install_protoc_windows.ps1`, which downloads
official protoc 33.5, verifies a pinned SHA-256, expands it in runner temporary
storage, checks `libprotoc 33.5`, and appends its bin directory to GITHUB_PATH.
The downloaded official zip hash and `bin/protoc.exe`/protobuf include layout
were verified locally. The PowerShell execution requires Windows CI; local
Linux has no PowerShell installed.

Existing workflow contract assertions were updated for the artifact versions.
No application source, dependency manifest, or lockfile was changed.

## Verification

```bash
node --version
node scripts/tests/validate_ci_release_workflows.test.mjs
go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.7 -shellcheck= .github/workflows/unittest.yaml .github/workflows/release.yaml
npm --prefix web run build
npm --prefix controller-web run build
npm --prefix web test
npm --prefix controller-web test
git diff --check
```

Passed: workflow positive/negative contracts, Actionlint (shellcheck disabled),
both frontend builds, 193 worker frontend tests and 167 controller frontend
tests. Builds retain non-fatal bundle-size and dependency annotation warnings.
Full hosted CI, including Windows protoc execution and packaging, follows the
push; local checks do not establish successful release publication.
