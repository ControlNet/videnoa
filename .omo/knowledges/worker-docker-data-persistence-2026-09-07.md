# Worker Docker data persistence

The Worker image runs in `/app`. Its default configuration is `data/config.toml`,
so the container path is `/app/data/config.toml`. Mounting media at `/data` does
not persist this configuration. README previously showed models/cache/media
mounts but omitted the Worker data directory.

The documented server command now mounts `$PWD/data:/app/data`, along with models
and the TensorRT cache, and runs detached under the name `videnoa`. The default
workflow directory `data/workflows` is included in that mount. Host media is an
optional separate `/media` mount. Keep the same host data directory when replacing
containers; overrides of `--data-dir` or `VIDENOA_DATA_DIR` require a corresponding
mount. Controller data remains separately mounted at `/workspace/data`.

README and Dockerfile server usage comments were updated. Runtime Dockerfile
instructions and application code are unchanged. Validation: inspect the command
against image WORKDIR and config defaults; `git diff --check` must pass. This is
a documentation-only correction; no GPU/container execution or Rust tests needed.
