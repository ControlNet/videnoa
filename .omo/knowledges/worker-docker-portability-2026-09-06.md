# Worker Docker portability

- The release workflow publishes controlnet/videnoa with version and latest tags.
- Live inspection on 2026-09-06 using `docker buildx imagetools inspect controlnet/videnoa:latest` confirmed a linux/amd64 image. The unknown/unknown entry is an attestation.
- Current Dockerfile bundles CUDA 12.6.3 with cuDNN, ONNX Runtime 1.23.2, and TensorRT 10.7.0. Users do not need to build on their target GPU.
- Linux hosts need a compatible NVIDIA driver and Docker configured with NVIDIA Container Toolkit; a host CUDA Toolkit installation is unnecessary.
- GPU support depends on bundled runtime versions. TensorRT 10.8 introduced Blackwell / GeForce 50-series support, so the current 10.7 stack cannot promise that support. Rebuilding the unchanged Dockerfile does not upgrade it.
- TensorRT engine generation occurs at runtime; keep engine caches specific to compatible target hardware/software. Model files must still be supplied.
- GPU passthrough smoke check: `docker run --rm --gpus all controlnet/videnoa:latest nvidia-smi`. This does not validate model inference compatibility.

Sources:
- https://github.com/NVIDIA/nvidia-container-toolkit/blob/main/README.md
- https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html
- https://docs.nvidia.com/deeplearning/tensorrt/10.x.x/getting-started/release-notes-10/10.8.0.html

README now prioritizes the Docker Hub worker image, lists host prerequisites, uses absolute bind mounts for models/cache and a home media directory, and retains local build as an optional alternative.

README section order prioritizes Docker usage and configuration before development setup. Source-build requirements are nested under development setup.
