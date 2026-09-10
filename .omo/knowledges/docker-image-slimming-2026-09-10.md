# Docker Image Slimming (2026-09-10)

## Implemented low-risk reductions

- The repository-root `.dockerignore` excludes local build outputs, runtime data,
  model/runtime libraries, media, frontend dependency/build directories, agent
  state, and common secret files from both Docker build contexts.
- The Worker runtime no longer copies `/app/web/dist`; release builds serve the
  same frontend from the assets embedded by `rust-embed`.
- The Worker keeps `mkvpropedit` but removes the unused `mkvextract`, `mkvinfo`,
  and `mkvmerge` executables in the same layer that installs MKVToolNix.

## Symbol retention decision

- Stripping was evaluated and then reverted. It saved about 13 MB of unpacked
  size per binary but only about 1.7 MB after gzip compression.
- Release binaries retain their symbol tables so production panic backtraces,
  core dumps, and offline address symbolization remain useful.
- The source contract rejects future `strip` commands in either Dockerfile.
- The retained-symbol QA images measured 5,815,081,126 bytes for the Worker and
  133,269,696 bytes for the Controller. The remaining Worker changes remove
  about 20 MB of unpacked payload from the duplicate WebUI and unused MKV tools.

## Measured build context

- The Docker build context observed during the Worker build was 4.54 MB. The
  Controller build transferred a 45.17 kB incremental context.

## Verification contract

`scripts/tests/docker_slimming_contract_test.sh` validates the source contract.
When passed a Worker image tag, it also verifies the runtime filesystem and starts
the server without a GPU to prove that health and the embedded WebUI remain
available. The Controller continues to use
`scripts/check_controller_container.sh <image> --all` for its full contract.

The larger CUDA runtime dependency reduction remains separate work and requires
representative CUDA inference plus cold and warm TensorRT engine-cache validation.
