# Controller version alignment

- Checked on 2026-09-06: root `Cargo.toml` declares workspace version `0.1.2`.
- `crates/controller/Cargo.toml` and `crates/app/Cargo.toml` both inherit it with `version.workspace = true`; the app defines the `videnoa` binary.
- `Cargo.lock` and offline Cargo metadata report `0.1.2` for all four workspace packages.
- Controller CLI enables Clap's package version. Linux and Windows packaging scripts read the root manifest version and validate the binary version.
- `controller-web/package.json` is a private frontend package with its own version `0.0.0`; it does not inherit the Cargo workspace version.
- Follow-up: the main frontend `web/package.json` also sets `private: true` and `version: "0.0.0"`. Both frontend lockfiles use `0.0.0` for the root package.
- Both frontend build scripts run `tsc -b && vite build`; neither Vite config injects a release version from Cargo or package metadata.
- `.github/workflows/release.yaml` resolves the release version from Cargo manifests and checks all four Rust crates for agreement. The frontend npm package versions are not part of that version gate.
- Main frontend assets are built by `scripts/package_dist.sh` and embedded from `web/dist` in `crates/core/src/server/mod.rs`. Thus Controller's npm version convention matches the main frontend's existing convention.
- This check covers source metadata, not deployed binaries or published release artifacts.

Verification command: `cargo metadata --no-deps --format-version 1 --offline`.
Expected result: all workspace package versions are `0.1.2`.

## Controller Docker release configuration

- `.github/workflows/release.yaml` defines `controller-dockerhub-publish`, dependent on the version and quality gates, using `Dockerfile.controller` and `push: true`.
- Image tags are `controlnet/videnoa-controller:<Cargo release version>` and `controlnet/videnoa-controller:latest`.
- Release triggers on pushes to `master` or manual dispatch; the publish gate skips non-master refs and existing version tags unless the master run uses the force override.
- GitHub Release depends on Controller image publication. Release verification pulls both Controller image tags.
- Ordinary CI in `.github/workflows/unittest.yaml` builds and smoke-tests a local `videnoa-controller:ci` image.
- These findings describe checked-out workflow configuration, not evidence of a completed remote publication.
