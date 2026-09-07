# Minimal Controller Docker README

- The root README now documents Controller under Docker with a short purpose, runnable image command, setup URL, and persistence/media-path explanation.
- Removed the earlier standalone Controller overview and guide-link list from the root README.
- Command uses host UID/GID, port 3001, a dedicated `./controller-data` bind mount at `/workspace/data`, and the user's media directory at `/media`.
- Dockerfile.controller confirms the working directory `/workspace`, port 3001, and default listener override to `0.0.0.0`.
- `README-controller.md` remains available for archive packaging; this change is limited to the root README.
- `git diff --check` and `bash scripts/tests/controller_docs_test.sh` passed. No containers were started for this documentation-only change.
