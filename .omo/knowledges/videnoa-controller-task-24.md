# Videnoa Controller Task 24

## CI and Release Contracts

- Keep the existing Videnoa Rust/web/package/GPU-image jobs unchanged and model Controller checks as independent jobs whose failures block the reusable release quality gate.
- Run Controller release-oriented Rust jobs on Rust 1.83. Serialize each frontend job after one `npm ci`; do not overlap standalone frontend commands with release Cargo builds that invoke the same frontend build script.
- Reuse `package_controller.sh`, `package_controller.ps1`, and `check_controller_container.sh --all`; these scripts own version, platform, exact-content, and GPU-free validation.
- Release completeness is a graph property: GitHub release creation must need both legacy packages/images and both Controller packages/image, use `fail_on_unmatched_files`, and verify every exact asset/tag after publication.
- Parse workflow YAML and validate job references/cycles, required dependencies, legacy preservation, exact assets, exact tags, and credential names. Mutation tests should remove one contract at a time and require a nonzero validation result.

## Verification Boundary

- Linux can execute the Controller archive and full image/content/runtime checks. Windows PE build, executable version proof, and ZIP creation require `windows-latest` and must not be claimed from Linux static checks.
- Local validation cannot truthfully push Docker Hub tags or create a GitHub release. The deterministic local proof is parsed action wiring plus reusable package/container smoke; hosted publication remains the release workflow's responsibility.
