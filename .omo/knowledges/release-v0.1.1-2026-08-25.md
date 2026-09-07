# v0.1.1 release evidence

## GitFlow topology

- Canonical version source: root `Cargo.toml` at `[workspace.package].version`.
- Release branch: `release/0.1.1` from `dev`.
- Version commit: `842caec Prepare v0.1.1 release`.
- Master merge: `20e4ef3 Merge branch 'release/0.1.1'`.
- Develop merge: `2e2920e Merge branch 'master' into dev`.
- The release workflow must create and push `v0.1.1`; pushing the tag before `master` would make the version gate skip publishing.

## Verification

- Release-branch CI: run `32847902383`, all Rust, Web, Linux/Windows package smoke, and Docker smoke jobs passed.
- Master release workflow: run `32850710277`, all quality, packaging, Docker publish, GitHub release, and release verification jobs passed.
- Dev post-merge CI: run `32850822664`, passed.
- GitHub release: `https://github.com/ControlNet/videnoa/releases/tag/v0.1.1`.
- Assets: `videnoa-linux64-0.1.1.7z.001`, `videnoa-win64-0.1.1.7z.001`.
- Docker verification pulled both `controlnet/videnoa:0.1.1` and `controlnet/videnoa:latest` successfully in the release workflow.

## Release caveats

- `cargo fmt --all -- --check` fails on the committed `crates/app/src/lib.rs`; the preserved user-owned local diff is exactly the rustfmt output and was intentionally not included in this minimal release.
- `npm ci` reports 16 vulnerabilities: 2 low, 2 moderate, 11 high, and 1 critical. The selected minimal release deferred dependency upgrades.
- GitHub Actions warns that `actions/checkout@v4` and `actions/setup-node@v4` target deprecated Node.js 20 action runtimes and are currently forced onto Node.js 24.
- The CLI currently has no `--version` option. `--help` works and invalid arguments exit with status 2.
