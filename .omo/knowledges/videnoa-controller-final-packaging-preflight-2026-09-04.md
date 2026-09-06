# Videnoa Controller Final Packaging Preflight

## Durable Findings

- `scripts/package_dist_archive.sh create` must verify that p7zip emitted either the requested unsplit `.7z` file or the split `.7z.001` first volume. Process exit zero alone is not a sufficient archive-creation contract.
- The focused legacy helper regression is `bash scripts/tests/package_dist_archive_test.sh`. It covers split and unsplit creation, missing verification output, successful creation without output, insufficient disk, and fatal `7z` propagation.
- Linux Controller packages are deterministic when built with `SOURCE_DATE_EPOCH=0`. The current `v0.1.2` real-binary archive produced identical SHA-256 `11a9196e6edeffc04ad86527a8f63dbd5355876dccb0571dd1b4a6bd03cc8bb2` in two independent output directories.
- The Controller Linux archive contract is exactly one versioned root containing `LICENSE`, `README-controller.md`, `controller.example.toml`, and `videnoa-controller` in deterministic order. Model, GPU/runtime, cache, loose frontend, target, library, key, and certificate content is forbidden.
- `bash scripts/check_controller_container.sh <image> --all` is the complete local image contract. It verifies source/image isolation, non-root execution, health, embedded SPA, writable mounts, restart persistence, and representative configuration/path errors.
- The validated `Dockerfile.controller` image is independently GPU-free: no legacy binary, models, Node/npm, ONNX Runtime, CUDA, cuDNN, TensorRT, NVIDIA packages, or caches were present.
- `node --test scripts/tests/validate_ci_release_workflows.test.mjs` protects both the complete positive release DAG and negative mutations. The contracts reject bypassing `scripts/package_dist_archive.sh` in legacy Linux smoke or release packaging.
- Secret review should combine staged, tracked, `.gitignore`, and a temporary mirror of every modified/untracked path. This run scanned 98 modified/untracked files with zero findings; two repository-wide detections were unchanged test fixtures outside the diff.
- Broad workspace timing failures are not packaging defects when the exact failing target and its complete integration target pass focused reruns. Preserve the raw observation in evidence instead of changing unrelated runtime code.
- Native Windows execution and real GitHub/Docker Hub publication remain external boundaries. Local static packaging and parsed workflow contracts must not be described as hosted publication success.
- Exact-SHA hosted reruns remain authoritative for required release gates: run `33827264003` passed strict Controller Clippy but failed `task20::multi_worker::three_worker_real_http_pipeline_uses_all_capacity_without_duplicates` because the mock observed two `Run` requests instead of one. Passing local reruns do not establish that such a hosted product-test failure is infrastructure-only.

## Evidence

- Complete packaging/release transcript and verdict: `.omo/evidence/videnoa-controller/final-preflight/packaging-release.txt`
- Lane verdict: PASS
