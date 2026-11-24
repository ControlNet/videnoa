# ONNX Runtime GitHub repository removal (2026-11-24)

## What happened

- Around 2026-11-17/18 the `microsoft/onnxruntime` repository disappeared
  from GitHub together with its release assets: the repository and every
  `releases/download/...` URL answer 404, with no redirect. Microsoft had not
  explained it as of 2026-11-24. PyPI (`onnxruntime-gpu`) and NuGet
  (`Microsoft.ML.OnnxRuntime.Gpu*`) packages are still published.
- Effect on Videnoa: only `Dockerfile` (stage `ort-download`) downloads
  `onnxruntime-linux-x64-gpu-${ORT_VERSION}.tgz` from GitHub, so the CI job
  "Docker build smoke" fails from 2026-11-21 on and a release would fail at the
  Docker image. Native Linux/Windows packaging does not download ONNX Runtime.
- The owner chose to wait for Microsoft to restore the repository rather than
  change the download source.

## Verified fallback (not applied)

NuGet `Microsoft.ML.OnnxRuntime.Gpu.Linux` 1.23.2 (Microsoft-signed,
`.signature.p7s` in the package):

```bash
curl -fsSL -o ort.nupkg \
  https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime.gpu.linux/1.23.2/microsoft.ml.onnxruntime.gpu.linux.1.23.2.nupkg
echo "590c4235f791e8a50ded4a049a4ba24ab1f88b296f93b2283ed9aab50395f5aa  ort.nupkg" | sha256sum -c
unzip -q ort.nupkg 'runtimes/linux-x64/native/*'
```

- `runtimes/linux-x64/native/` holds `libonnxruntime.so`,
  `libonnxruntime_providers_cuda.so`, `..._shared.so` and `..._tensorrt.so`.
  All four are byte-identical (sha256) to the files of the GitHub 1.23.2
  `gpu` tgz used for the local `lib/` and the Docker image.
- Layout difference: the tgz ships `libonnxruntime.so.1.23.2` with
  `libonnxruntime.so.1` and `libonnxruntime.so` symlinks; NuGet ships only
  `libonnxruntime.so` (SONAME `libonnxruntime.so.1`). To keep the image
  identical, rename it to `libonnxruntime.so.${ORT_VERSION}` and recreate both
  symlinks; `scripts/tests/docker_slimming_contract_test.sh` checks
  `/usr/local/lib/libonnxruntime.so.1.23.2`.
- The `ort-download` stage would also need `unzip`, and the contract test
  pins `ARG ORT_VERSION=1.23.2` and the download step.
- Because the binaries match, the ONNX Runtime 1.23.2 libraries Videnoa
  already ships are the ones Microsoft also publishes on NuGet.
