# Binary deployment for iroh testing — 2026-09-07

Release builds embed both WebUIs; copying their dist directories or installing
Node on the destination is unnecessary. Build the two server packages with
`cargo build --release --locked -p videnoa-app -p videnoa-controller` to avoid
building the desktop application's additional native dependencies. --workspace
also works when all desktop prerequisites are installed. Standard outputs are
`target/release/videnoa` and `target/release/videnoa-controller`, unless a custom
Cargo target directory/target triple is configured.

Destination OS, CPU architecture and dynamic system-library versions must match
the binaries. Check with ldd on a Linux destination; it cannot detect missing
libraries loaded dynamically later, such as ONNX Runtime.

Worker startup configures/discovers runtime libraries but does not require model
inference merely for health/iroh connection testing. Actual video processing
requires applicable models, workflows/presets, FFmpeg/ffprobe, ONNX Runtime and
GPU execution-provider libraries/drivers. Controller has no GPU/model dependency.
Presets are external data, unlike embedded frontend assets.

Launch worker and Controller in separate writable working directories. Worker
supports --data-dir; Controller uses <working-directory>/data. Fresh instances
create their own persistent configuration/database state; iroh identities live
there as well. Do not duplicate an active instance's identity for a new test peer.
Set the worker password, enable iroh, copy Endpoint ID, then register that ID and
password in Controller. Inbound public Worker HTTP port forwarding is unnecessary
for iroh; initial browser setup still needs access to each service's WebUI, and
both hosts require outbound connectivity for N0 discovery/relay services.

## Presets warning clarification

Worker `load_builtin_presets` directly reads `config.paths.presets_dir`, default
`presets`, relative to the process working directory (not the executable directory
and not --data-dir). If missing, it logs the warning and returns an empty preset
map; this does not by itself disable iroh or health, but bundled workflows are absent.
Release WebUI embedding does not embed preset JSON.

Current Dockerfile copies presets into /app/presets and uses WORKDIR /app.
Both Worker release packagers include presets: package_dist.sh line 463 and
package_dist.ps1 line 725 copy the repository presets directory into the bundle.
The release workflow archives that bundle as .7z. These are packaging-source
checks, not inspection of every historical published asset. Standard Worker bundle
also includes lib, bin, models, the worker/desktop executables, README and LICENSE.
The standalone Controller archive intentionally does not include worker presets.

For a binary-only test deployment, additionally copy repository presets/ into the
Worker launch directory, or configure an absolute [paths] presets_dir. Keep the
full release bundle for real video processing and replace only its Worker binary
with the compatible new build if testing this branch. No preset data needs to be
copied to Controller. Prior binary-only advice applies to transport reachability,
not a warning-free or complete video-processing deployment.

## Controller preset discovery diagnosis

Controller requests both /api/workflows and /api/presets, so a preset need not be
copied into data/workflows to be discovered. Only compatible entries are retained
in worker capabilities: workflow.interface.inputs must include both input and
output ports of type Path. All six current repository presets meet this rule.
Health/capability cache TTL uses the configured health cadence (default 10 seconds),
not indefinite caching. Authentication-blocked workers require a registration edit;
restarting only the worker does not clear the Controller's in-memory version block.
Worker JSON preset loading is nonrecursive, occurs at startup, and logs Loaded
preset on success or Failed to parse/read preset on failure. Missing models are
not part of Controller's interface eligibility check.
Remote diagnosis requires worker Online/Offline/last_error and evidence from the
worker's own preset UI or authenticated /api/presets response; without those,
missing path, admission failure, schema mismatch and UI refresh cannot be
conclusively distinguished. No remote state was inspected in this assessment.

## Bundled ffprobe shared-library failure

A remote deployment reported ffprobe exit 127 because libavdevice.so.58 could not
be loaded. This occurs before media probing and is independent of iroh transport.
Current runtime::command_for locates bundled executables but does not set a
Linux child-process library search path. Parent-process RTLD_GLOBAL preloading
for ONNX Runtime does not carry loaded libraries through exec into ffprobe.

The packager extracts misc bin/lib assets. validate_linux_runtime_libs checks
cuDNN presence only; check_linux_package_compat runs videnoa --help and inspects
glibc symbol requirements on Worker/desktop, without executing bundled ffprobe
or ffmpeg. These checks cannot establish a self-contained FFmpeg dependency set.

The error alone does not distinguish missing library files, broken versioned
symlinks, and an unconfigured loader path. Inspect remote lib/bin for
libavdevice.so*, run ldd bin/ffprobe, and try bin/ffprobe -version with an explicit
LD_LIBRARY_PATH pointing to the bundle lib directory. If that succeeds, search
path configuration is implicated; if it still fails, inspect remaining missing
SONAMEs/dependencies. Do not alias incompatible FFmpeg major SONAME versions.
No remote files or release asset binaries were inspected in this assessment.
