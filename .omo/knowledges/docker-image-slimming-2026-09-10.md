# Docker Image Slimming (2026-09-10)

## Implemented low-risk reductions

- The repository-root `.dockerignore` excludes local build outputs, runtime data,
  model/runtime libraries, media, frontend dependency/build directories, agent
  state, and common secret files from both Docker build contexts.
- Both release binaries are stripped in a separate builder layer. Keeping the
  Cargo build command unchanged preserves its existing Docker cache key.
- The Worker runtime no longer copies `/app/web/dist`; release builds serve the
  same frontend from the assets embedded by `rust-embed`.
- The Worker keeps `mkvpropedit` but removes the unused `mkvextract`, `mkvinfo`,
  and `mkvmerge` executables in the same layer that installs MKVToolNix.

## Measured results

- `videnoa-controller:slim-qa`: 121,373,776 bytes, down from 133,269,488 bytes.
- `videnoa:slim-qa`: 5,803,568,734 bytes. This is 4,875,426 bytes below the
  local v0.1.3 image despite the current v0.1.4/Iroh Worker binary being larger.
- The stripped Worker binary is 32,500,080 bytes.
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
