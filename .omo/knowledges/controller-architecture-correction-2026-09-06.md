# Controller configuration, media paths, and origin correction

This correction starts from `origin/dev` at `d94b2d66884d25859de4872258423dbb747a9f50`. It preserves the Task API low-cost test PHC, real login, reusable
cookie/CSRF fixtures, constrained request-phase SQLite pool, and production-cost
authentication stress coverage. No old migrations, Cargo dependencies, project
rules, frontend implementation, or production password parameters are changed.

## Configuration ownership and runtime application

1. `data/controller.toml` is the sole persisted Controller configuration source.
   Startup parses and validates it, creates defaults when missing, and installs
   the resulting `ControllerConfig` in the shared `ConfigManager`.
2. Active SQLite configuration reads/writes are removed: server, cookie/session
   policy, pause, compute/prefetch/transfer limits, timeouts, retry policy,
   `config_document`, `pending_config_document`, `configuration_initialized`, and
   durable configuration version CAS. Legacy schema columns remain physically
   present because historical SQL migrations are unchanged. Even legacy pending
   documents are ignored; existing TOML wins. No bidirectional synchronization or
   projection repair runs.
3. SQLite still owns tasks, attempts, workers, task history, idempotency,
   remote identity, retry/recovery evidence, administrator Argon2id credential,
   and digest-only authentication sessions. These are operational/security state.
   Configuration DTOs now live in the config module; persistence retains public
   reexports for existing embedded callers.
4. Settings validates policy and prebinds/reserves listener handoff before the
   durable write. It holds exclusive admission, writes and fsyncs private TOML,
   atomically replaces `controller.toml`, fsyncs `data`, then changes the runtime
   snapshot, independent transfer limits, timeout/retry consumers, auth policy,
   and listener. TOML failure leaves runtime and generation unchanged. If the
   listener stops after preflight, the error explicitly says settings were saved
   and restart loads them. There is no database projection journal.
5. Manual TOML edits take effect after restart. No watcher or polling exists.
   Shutdown blocks admission in memory without saving an implicit pause, so it
   cannot overwrite a manual edit. Explicit Web pause remains persisted across
   restart. A crash after successful TOML replacement naturally recovers from TOML.
6. Settings uses an in-memory generation; stale requests conflict. Generation
   resets at process startup. `--host` and `--port` persist directly to TOML,
   preserving the existing intended CLI override behavior.
7. ConfigManager owns the shared admission read/write lock. Reservations and
   replacement compute retries hold read admission through their SQL transaction;
   upload intake also holds it. New Submit retains admission through remote
   acceptance and evidence persistence. Settings/shutdown acquire write admission.
   SQL receives runtime pause/prefetch values instead of joining legacy settings.
   Existing processing, downstream download/verify/publish/cleanup, independent
   upload/download pools, and capacity/prefetch behavior remain intact.

## Filesystem and publication

8. Absolute task paths remain in the process-visible filesystem namespace, even
   outside workspace. They use descriptor-backed filesystem anchors and the
   existing no-follow component traversal and identity checks.
9. Relative task paths resolve from Controller workspace (startup working
   directory). Intake persists normalized absolute paths; extensions may differ.
   Parent traversal remains rejected.
10. All of `workspace/data/**` remains private, including config, database,
    sidecars, transient artifacts, and future private files. The boundary also
    checks retained private directory identity and rejects symlink aliases.
    Windows comparisons handle case and ordinary/verbatim drive/UNC spellings;
    ambiguous trailing-dot/space components and alternate data streams are
    rejected. Non-regular input is rejected before open; nonblocking no-follow
    open also prevents a racing FIFO replacement from blocking intake. Linux tests cannot execute native Windows filesystem operations.
11. Output outside workspace can finalize through the existing atomic no-replace
    rename when its mount relationship supports that operation. It is never
    rebased under workspace or replaced with another destination.
12. Different devices fail output intake with `PathError::CrossFilesystemPublication`.
    Rename `EXDEV` (including separate mounts on one device) produces the same
    typed path/publication error; persisted task failure uses `publication_failed`
    with an explicit cross-filesystem message. No copying fallback is installed.
13. Verified bytes stay in Controller-private storage until final rename. No
    sibling `.videnoa-*`, `.partial`, `.staging`, or incomplete final file is
    created. Existing outputs are never overwritten. Crash-after-rename recovery
    recognizes final evidence and completes without another AI invocation.

## Origin and Docker

14. Parsed Origin/Host authorities must match, including normalized default
    ports, hostname, IPv4, or bracketed IPv6. `secure_cookie=false` accepts HTTP
    and HTTPS. `secure_cookie=true` accepts HTTPS and rejects HTTP. Missing,
    malformed, duplicate, credential-bearing, or foreign Origin proof is rejected.
    Forwarded headers are not trusted; existing CSRF checks remain mandatory.
15. Default HTTPS first-access setup works. An HTTPS session can save the change
    from non-Secure to Secure cookies. Existing policy-fingerprint behavior still
    invalidates old sessions; a fresh real login receives a Secure cookie, and
    subsequent mutations must supply HTTPS same-origin proof and CSRF.
16. Docker listener controls remain available with no detection or port disabling.
    Docs explain container-visible paths and a common parent mount with a separate
    Controller working directory outside media. Independent bind mounts can
    produce EXDEV. Listener port changes require updating port publishing, proxy,
    and health checks.

## Direct regression evidence

| Corrected contract | Tests |
| --- | --- |
| Missing defaults, existing valid policy, malformed/private TOML | `config_bootstrap`, `config_defaults_contract`, `config_contract` |
| TOML wins over poisoned legacy config; no reload; restart/crash; stale generation; durable Web pause; graceful shutdown preserves manual edits | `config_persistence` |
| Full Web hot apply, immutable legacy SQL row, TOML failure rollback, listener capability validation | `operations/settings/tests.rs` |
| Persisted TOML pause waits for admitted Submit | `scheduler/service.rs::settings_update_waits_for_admitted_submit` |
| Candidate/reservation/upload SQL ignores poisoned legacy pause/prefetch | `task11::scheduler::prefetch_is_bounded_and_idle_uploads_precede_optional_prefetch` |
| Absolute external task paths and differing extensions; relative-to-workspace absolute persistence | `task_api::intake_contract` |
| Private paths, symlinks, changed input, existing output, replaced external parents, different filesystem intake | `workspace_paths` plus retained `path_capabilities` and filesystem fault suites |
| No visible intermediate; external final publication; typed real EXDEV; no overwrite | `paths/publication_tests.rs` |
| External crash after rename preserves compute identity | `task13::publication::external_output_crash_after_rename_recovers_without_ai_replay` plus retained `task13` publication/recovery suites |
| Origin transport matrix; authorities, ports, IPv4/IPv6, malformed origins | `auth/boundary.rs` unit tests |
| Default HTTPS first setup | `auth_bootstrap::default_first_access_setup_accepts_https_reverse_proxy_origin` |
| HTTPS Settings Secure transition, old-session rejection, fresh Secure login and HTTPS/HTTP mutation proof | `task_api::intake_contract::https_session_can_enable_secure_cookies_then_requires_https_proof` |
| Real extracted archive startup, Settings CAS/TOML, external/private paths, graceful restart, retained tasks/sessions | `scripts/tests/controller_architecture_smoke.py` |
| Docker external media namespace, startup/setup/restart, private-path rejection | `scripts/check_controller_container.sh --all` |

New filesystem/mock fixtures are explicitly synthetic and isolated. Authentication
fixtures continue using the existing explicitly test-only PHC; no unrelated Task
API request adds production-cost Argon2 verification. Native Windows runtime
verification requires the Windows CI environment.

## Verification commands and observed results

The following commands passed on Linux with Rust 1.83.0:

```bash
cargo +1.83.0 fmt --all -- --check
cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.83.0 test --locked -p videnoa-controller --test auth_contention -- --ignored --nocapture
```

The explicit production-cost authentication stress test passed in 2.86 seconds:
eight Bearer verifications, 227 heartbeat ticks, maximum gap 10.46 ms against a
274.49 ms calibrated bound. The existing production Argon2 parameters and normal
parallel test execution are unchanged.

Controller Web validation passed (132 unit tests across 22 files):

```bash
cd controller-web
npm run lint
npm test
npm run build
npm run test:e2e -- --project=chromium tests/e2e/shell.spec.ts tests/e2e/operations.spec.ts tests/e2e/task-creation.spec.ts
```

Chromium passed all 12 selected setup/login, Settings/worker, and task creation
cases. The build reports only existing upstream Zod/Rollup annotation warnings.
No frontend implementation was changed.

Distribution and documentation checks passed:

```bash
bash scripts/tests/controller_docs_test.sh
bash scripts/tests/package_controller_test.sh
bash scripts/tests/controller_archive_root_files_test.sh
bash scripts/tests/package_controller_windows_static_test.sh
node scripts/tests/validate_ci_release_workflows.test.mjs
docker build -f Dockerfile.controller -t videnoa-controller:architecture-qa .
bash scripts/check_controller_container.sh videnoa-controller:architecture-qa --all
```

The final image was built from the modified production sources. Container smoke
passed source/image contracts, non-root startup, first-access setup, session
restart persistence, private-path rejection, and external media intake with
workspace in a separate sibling directory within one mounted filesystem.

For a reproducible release archive smoke (after the Docker build above):

```bash
artifact_dir=$(mktemp -d /tmp/controller-architecture-archive-XXXXXX)
container_id=$(docker create videnoa-controller:architecture-qa)
docker cp "$container_id:/usr/local/bin/videnoa-controller" "$artifact_dir/videnoa-controller"
docker rm "$container_id"
bash scripts/package_controller.sh --binary-path "$artifact_dir/videnoa-controller" --output-dir "$artifact_dir"
mkdir "$artifact_dir/extracted"
tar -xzf "$artifact_dir/videnoa-controller-v0.1.2-linux-x86_64.tar.gz" -C "$artifact_dir/extracted"
python scripts/tests/controller_architecture_smoke.py --binary "$artifact_dir/extracted/videnoa-controller-v0.1.2-linux-x86_64/videnoa-controller"
```

The real extracted archive smoke passed startup, HTTPS first setup, Secure-cookie
transition and session invalidation, new Secure login, HTTP mutation rejection,
Settings conflict/TOML persistence, external/private media paths, unchanged legacy
SQL config fields, graceful restart loading a manual edit, and retained tasks and
sessions. The smoke script uses only Python standard-library modules and isolated
synthetic data; it requires no Python package installation or environment change.

`git diff --check` and the staged Secret Guard scan passed. Old SQL migrations,
Cargo profiles/lockfile, production password cost, and project AGENTS.md are intact.

The final full Rust command also passed on the completed production code:

```bash
cargo +1.83.0 test --locked -p videnoa-controller --all-targets
```

444 tests passed across 49 binaries, zero failures, and one intentionally
ignored production-cost stress test (run separately above). Summed test execution
was 389.38 seconds with default libtest parallelism. This includes all
Controller fault/recovery, capacity/prefetch, upload/download, cancellation,
publication, authentication, load, and security suites. No global single-thread
requirement or weaker authentication fixture was introduced.
