# Learnings

## 2026-09-02 Task 1

- A release-only Controller build script can remain GPU-free while following the existing frontend convention: resolve `controller-web/` from `CARGO_MANIFEST_DIR`, run `npm ci --no-fund`, run `npm run build`, and let `rust-embed` consume `dist/` only under `not(debug_assertions)`.
- Modeling frontend assets as a validated public wrapper over a private cfg-specific source keeps debug directory paths and release embedding type-safe without lint suppression.
- `tower_http::ServeDir` plus `ServeFile(index.html)` provides debug static serving and nested-route SPA fallback after startup validates the directory and index.
- A package-level `cargo tree --edges normal,build` contract test is a fast guard against accidental `videnoa-core`, ORT, CUDA, cuDNN, TensorRT, or model coupling.

## 2026-09-02 Task 1 Fix Round 1

- Cargo 1.83 rejects a workspace member using edition 2024 during metadata parsing, so the isolated Controller must match the neighboring edition 2021 crates while the repository MSRV remains 1.83.
- Reserving `/api/{*path}` before the static fallback preserves nested SPA routes while preventing unknown API requests from returning HTML with status 200.
- Browser-resolved canvas sampling measured dark `oklch(0.61 0.22 292)` at `4.6943:1` against the Controller surface for both 12px and 14px normal text.

## 2026-09-02 Task 1 Fix Round 2

- Axum 0.8 catch-all routes such as `/api/{*path}` require a non-empty wildcard and therefore do not match `/api` or `/api/`; exact routes are required to keep both API roots out of the SPA fallback.
- A cfg-specific `TestAssets` fixture lets the same health, API-boundary, and SPA contracts run against disk assets in debug builds and embedded assets in release builds.

## 2026-09-02 Task 1 Fix Round 3

- Axum route matching preserves percent-encoded path spellings, so a strict single-decoding middleware boundary is required before SPA fallback selection.
- `percent_decode_str` decodes valid escapes but leaves malformed escapes unchanged; validating every `%` triplet first and rejecting invalid UTF-8 produces a deterministic 400 boundary.
- Wrapping both disk and embedded SPA fallbacks in GET routing preserves automatic HEAD support while consistently returning 405 for POST and OPTIONS.

## 2026-09-02 Task 1 Fix Round 4

- Debug `ServeDir` decodes safe percent escapes for filesystem lookup, but release `rust-embed` lookup does not; preserving the one-pass decoded path in a typed request extension removes that profile-specific seam.
- Decoded paths must reject backslashes and C0 controls after strict percent/UTF-8 validation so unsafe values cannot reach either static fallback.
- Live debug and release probes confirmed canonical and encoded-hyphen JavaScript paths return identical bytes, status, and `text/javascript` content type.

## 2026-09-02 Task 1 Fix Round 5

- `http::Uri::path()` preserves raw dot and repeated-separator spelling while excluding the query; Controller must therefore parse decoded segments explicitly before routing.
- Debug `ServeDir` ignores exact `.` and repeated separators during filesystem component parsing, while release `rust-embed` performs literal key lookup. Shared rejection is safer than profile-specific normalization.
- Browser URL parsing canonicalizes dot segments before transmission, so raw canonical-path policy requires `curl --path-as-is` evidence in addition to browser asset/SPA QA.

## 2026-09-02 Task 1 Fix Round 6

- A filesystem `Path` cannot preserve a meaningful final empty component, so debug exact lookup must branch on the validated `DecodedPath` trailing separator before joining it to the asset directory.
- Disk and embedded fallbacks now share one policy: an exact file key is attempted only for non-empty decoded paths without a trailing separator; otherwise the actual SPA index is returned.

## 2026-09-02 Task 1 Fix Round 7
- Host `Path` semantics cannot validate foreign-platform prefixes; validate every URL component before constructing a disk path.
- Windows drive/ADS punctuation and DOS device basenames must be exact-asset ineligible while remaining valid SPA routes.

## 2026-09-02 Task 1 Fix Round 8

- Windows recognizes `COM¹`-`COM³` and `LPT¹`-`LPT³` as device aliases; UTF-8 byte length is therefore not a valid classifier for numbered device names.
- `CONIN$` and `CONOUT$` follow the same case-insensitive basename-before-extension rule as other reserved devices.
- Cross-platform request tests should use safe frontend roots; real special-name mutation fixtures belong in an explicit Unix-only test.

## 2026-09-02 Task 2

- Figment layering is deterministic when providers are ordered as serialized defaults, exact TOML, then `VIDENOA_CONTROLLER_` environment values split on `__`; `deny_unknown_fields` also rejects unknown prefixed environment keys after extraction.
- Keeping raw configuration numerics as `u64` until one validation boundary produces dedicated zero and overflow errors instead of relying on lossy casts or parser-specific messages.
- `url::Url` supplies a normalized host/scheme/default-port representation; Controller additionally rejects credentials, query strings, and fragments and normalizes the base path to one trailing slash.
- Request-only secret DTOs can derive `Deserialize` without `Serialize`, while a redacted secret newtype prevents accidental `Debug` disclosure.
- Deterministic contract evidence can round-trip fixed UUIDs and timestamps while recording only the login request field name, never a password, cookie, token, or CSRF value.
- Do not run the Controller release Cargo build concurrently with a standalone `controller-web/npm ci`: both mutate `controller-web/node_modules`, so frontend and release Rust gates must be serialized.

## 2026-09-02 Task 5

- Durable idempotency must elect the creator in SQLite before adding runtime maps or spawning execution; an in-memory check cannot close concurrent or restart windows.
- A partial unique index on non-null keys preserves existing unkeyed behavior while enforcing one durable keyed mapping.
- Recursive object-key sorting is sufficient for semantic JSON replay while preserving array order and scalar distinctions.
- Restart replay should expose the current persisted status, including the existing queued/running-to-cancelled startup reconciliation, rather than reconstructing the original queued response.
- Persistent worker `jobs.db` is part of the exactly-once evidence boundary; database loss is ambiguous and must never trigger blind resubmission.

## 2026-09-02 Task 3

- SQLite partial indexes need predicates and leading columns that the planner can prove from literal repository queries; queue and recovery indexes were shaped to avoid temporary ordering trees.
- Atomic idempotent ingress must rollback the candidate task when a key already exists, otherwise retries create durable orphan tasks.
- Capacity-aware reservation must check enabled and online worker state in the same conditional write that claims the queued task.

## 2026-09-02 Task 5 Follow-up

- Durable replay classification must precede every dependency needed only for first creation; otherwise later filesystem drift can mask the persisted answer.
- A read preflight and a transactional final claim serve different purposes: preflight preserves the replay contract, while the final `BEGIN IMMEDIATE` claim preserves exactly-one dispatch under races.

## 2026-09-02 Task 8

- Remote workflow names and file API paths are opaque server values; preserving exact spelling, including `..`, is separate from validating URL-safe API path components.
- Reqwest response-body failures need separate timeout, transport, malformed/truncated, and size-boundary classification because a successful response header does not imply a complete body.
- Capability discovery is deterministic when workflow interfaces outrank preset evidence, missing Path inputs are incompatible, and cache invalidation is explicit for TTL, health, restart, and remote errors.
- Rust 1.83 lockfile verification requires pinning the inactive QUIC dependency resolution used by reqwest rustls: `quinn 0.11.9` and `quinn-proto 0.11.13` avoid the Edition 2024-only `cpufeatures 0.3.1` manifest.
- Canonical JSON numbers must normalize equivalent integral and floating spellings such as `1` and `1.0`; array order and non-number scalar types remain distinct.

## 2026-09-02 Task 8 Preset Contract Follow-up

- `/api/presets` is the authoritative preset compatibility boundary: bundled IDs may be extensionless slugs, API-created IDs may be UUIDs, and each response already contains the complete workflow interface.
- `/api/workflows/{filename}/interface` remains the correct discovery path for saved workflows, but preset IDs must not be projected into that filename endpoint.
- Inserting workflows before presets through `CompatibilityCatalog::insert` preserves deterministic workflow precedence when both sources expose the same ID.

## 2026-09-02 Task 3 MSRV Scope Correction

- Cargo workspace package fields are opt-in defaults, not workspace-wide guarantees; only members declaring `rust-version.workspace = true` advertise that MSRV.
- The tracked lock proves Controller resolution under Cargo 1.83, but it does not make the pre-existing GPU core parseable because exact `ort-sys 2.0.0-rc.12` requires edition-2024 Cargo support.

## 2026-09-02 Task 4

- A capability API is not race-resistant when it checks `symlink_metadata` and then uses a symlink-following directory open; every component descent must use `open_dir_nofollow` so a swap between check and open fails closed.
- Relative configured roots can preserve the Task 2 configuration contract by resolving once against the process directory, then retaining an absolute display path and descriptor-backed directory.
- A retained directory descriptor stays safe after its pathname is replaced, but queued work must still fail because the configured root no longer names that descriptor; snapshot and revalidate root identity before filesystem use.
- Moving the authenticated server entrypoint into the auth HTTP module kept every touched production Rust module below the 250 pure-LOC ceiling without changing the exported API.

## 2026-09-02 Task 6

- Hyper validates a fixed body's exact size against an explicit `Content-Length`; claiming a larger length panics inside the connection task. An unknown-size stream that emits bytes and then errors creates the intended client-visible truncated body cleanly.
- Joining the accept loop before same-address rebind makes real connection refusal and recovery deterministic without timing sleeps.
- Generation-bearing `watch` checkpoints preserve reach/release events even when waiter polling is delayed.
- Redacting volatile transport headers produced identical evidence SHA-256 values across consecutive full harness runs.

## 2026-09-02 Task 9 Submitting Cancellation Follow-up

- Cancellation intent cannot resolve a keyed submission's acceptance ambiguity. The durable lifecycle needs an explicit reconciliation result before it can authorize the correct cleanup action.
- Accepted evidence must be bound in the same paired task/attempt CAS that exposes remote cancellation; confirmed non-acceptance must durably return the attempt to staged before local cleanup is exposed.
- Reconciliation is a privileged lifecycle seam: keeping ordinary advancement blocked after cancellation intent prevents accidental polling or submission continuation.

## 2026-09-02 Task 9

- Task and current-attempt lifecycle transitions must be one atomic persistence operation; separate public status mutation APIs make legal policy bypassable and permit split-brain rows.
- Persist-before-side-effect is clearest when the service returns a typed `DurableAction` only after CAS commit, with submission evidence bound in the same transaction that authorizes polling.
- Explicit processing retry needs terminal remote and workspace-cleanup evidence plus a new attempt/key, while downstream retry must preserve the existing attempt to avoid repeating successful compute.
- Ambiguity codes must outrank persisted retryability metadata, otherwise contradictory evidence can accidentally become retryable after restart.

## 2026-09-02 Task 10

- Startup reconciliation can remain scheduler-independent by scanning durable nonterminal rows and returning stage-typed commands only after any required lifecycle commit.
- Worker outage recovery must update worker health metadata without releasing task assignment; existing nonterminal capacity queries then preserve slot accounting automatically.
- Process-level shutdown verification needs both an unreachable listener after signal and the durable settings pause bit; either observation alone is incomplete.

## 2026-09-02 Task 10 Contract Repair

- Task-local durable corruption must be converted to nonretryable ambiguity inside the scan loop; only persistence/infrastructure failures may abort startup recovery.
- A cancellation marker is a recovery routing boundary, not a field ordinary stage dispatch may ignore.
- Remote job existence alone is insufficient evidence; workflow and exact input/output parameters must agree with the durable attempt.
- Tracking synthetic shutdown permits does not prove production safety; block an actual SQLite transition and observe the reconciler-held permit during drain.

## 2026-09-02 Task 11

- Scheduler selection and reservation are separate race windows, so pause, compatibility, capacity, and prefetch predicates must be repeated in the atomic task claim.
- Existing lifecycle/recovery fixtures that insert an online worker must also persist compatible capability evidence; `online` alone is no longer schedulable evidence.
- Recovery matrix fixtures need an explicit high prefetch setting when constructing many simultaneous durable states for one worker.
- A per-worker upload set layered over independent upload/download counters provides deterministic transfer admission and RAII release without coupling the two pools.

## 2026-09-03 Task 11 Review Fix

- Worker capacity reduction and task reservation are competing SQLite writes; the capacity predicate must be part of the worker update statement, not a service-level usage preflight.
- A failed conditional update can classify stale version versus capacity rejection inside the same write transaction because the SQLite writer lock prevents the observed version and assignment count from changing before commit.
- Tokio oneshot checkpoints can deterministically expose persistence TOCTOU windows by pausing one operation after its read and releasing it only after the competing durable write commits.

## 2026-09-03 Task 12

- Workflow paths and file API paths are separate namespaces: the Controller owns deterministic API targets, while the worker-returned opaque paths must be persisted unchanged for submission and recovery.
- SQLite timestamp persistence is millisecond precision, so descriptor-backed input mtime checks must compare at that boundary rather than treating sub-millisecond filesystem precision as drift.
- Requiring response `Content-Length` and hashing through an `AsyncWrite` wrapper provides bounded-memory length and SHA-256 evidence in the same streaming pass.
- A freshly truncated `.part` plus same-directory verified rename gives one restart model without Range support; failed bodies remove the partial and retry the existing attempt without replaying compute.

## 2026-09-03 Task 12 Review Fix

- A filesystem rename is not the end of the durability boundary: sync the containing directory, then make restart reconciliation hash and accept an already matching verified artifact before retrying network transfer.
- Retry authorization must compare both task and attempt durable deadlines; checking only one row can admit work early after a partially inconsistent historical write.
- Recovery commands are not production behavior until the startup composition root dispatches them and follows up tasks whose transfer transition exposed the next recovery stage.

## 2026-09-03 Task 12 Convergence Fix

- Remote `NotFound` while resuming `Uploading` is positive reconciliation evidence that permits a fresh PUT now; treating it like an outage creates an endless stat/retry loop.
- A generic lifecycle conflict can be safely deferred only after re-reading the exact durable predicate that explains it. Persisted pause plus `Reserved` identifies the admission race without hiding stale snapshots or attempt mismatches.
- Offline rename-before-CAS recovery requires local provenance, not merely local bytes. A synced fixed-size length/hash record lets restart accept matching bytes without stat/GET and deterministically reject corruption.

## 2026-09-03 Task 12 Windows Durability Review

- Cross-platform durability contracts should distinguish an actual Unix parent-directory fsync from Windows reliance on individually synced files plus same-directory rename; grouping both behind `not(unix)` incorrectly rejected successful Windows downloads.
- A host-independent policy function can lock cfg intent on Linux while platform-specific implementations remain responsible for real syscalls.

## 2026-09-03 Task 13

- Persisting a destination staging name in the same CAS that enters `Publishing` makes every later filesystem artifact attributable after a crash.
- Always copying into destination-owned staging removes EXDEV from finalization and gives same-filesystem and cross-filesystem inputs one recovery algorithm.
- Local publication and temp cleanup must not wait for worker health; an offline worker belongs to the durable remote-cleanup retry stage after the final output is safe.

## 2026-09-03 Task 13 Review Fix

- A no-replace syscall is insufficient when its path lookup is ambient; Linux finalization must use the same capability-opened parent descriptor for both rename operands.
- Publication recovery must classify filesystem node type before opening. Directories, FIFOs, devices, and other non-regular nodes are ownership ambiguity, not retryable I/O.
- Directory identities created during staging must be retained through finalization; validating only the nearest pre-existing parent does not detect replacement of newly created descendants.
- Startup transfer dispatch needs the same task-local corruption isolation as startup reconciliation so one malformed cleanup trace cannot prevent unrelated recovery work.

## 2026-09-03 Task 13 Final Convergence

- Verifying a staging handle before rename is not enough when finalization later resolves the leaf by name; rehash the final artifact before lifecycle CAS to bind advancement to the bytes actually published.
- Capability-backed directory handles may be opened as path-only descriptors on Unix. Open `.` relative to the retained directory with read/directory flags before `fsync` to preserve identity and obtain a sync-capable descriptor.
- Deterministic checkpoints around irreversible effects make real race and crash tests possible without sleeps or test-only production branches.

## 2026-09-03 Task 14

- A single bounded broadcast channel can satisfy active-state SSE without history replay when every connection starts with `refetch` and lagged receivers are converted to the same signal.
- Route-level processing retry can preserve lifecycle proof requirements by querying terminal remote state and converging deletion before constructing typed evidence; it must never infer those proofs from the local failed state alone.
- Readiness is more useful when it verifies durable migrations, current credentials, and retained filesystem capabilities while intentionally excluding worker liveness.
- Publishing durable change identifiers at the service boundary lets SSE resolve the latest active snapshot after commit and naturally covers scheduler, recovery, transfer, and worker-health paths.
- Long-lived authentication checks need both a persistent timer and a non-touching session validation path; otherwise traffic can defer checks or keep idle sessions alive.

## 2026-09-03 Task 12 Input Identity Regression

- Device, inode, byte length, and modification time are not a content identity: fast remove-and-recreate can reproduce all four values. Admission that must issue zero remote writes for changed bytes needs a durable content digest.
- Hashing a capability-opened input must rewind the same handle before transfer and re-read descriptor metadata after hashing so the accepted digest remains bound to the checked file state.

## 2026-09-03 Task 14 Live QA

- Interactive SSE verification should start the stream and trigger the durable mutation within one bounded shell transaction; otherwise tool latency can expire an otherwise healthy short-lived curl capture before the mutation occurs.
- A no-replay reconnect proof is strongest when the client first observes `refetch`, waits a bounded interval with zero historical delta events, and only then triggers a new mutation whose typed delta must arrive.
- Put generated Bearer authentication in a mode-0600 transient curl header file and reference it with `--header @file`; this keeps the value out of command output and process arguments while still exercising the raw HTTP boundary.

## 2026-09-03 Task 14 Oracle Blockers

- Retry cleanup proof is identity proof, not merely terminal-status proof: job ID, workflow, and exact durable input/output params must match before deleting remote workspace state.
- Aggregate enum endpoints should project sparse persistence results onto one explicit ordered variant table so empty categories remain part of the stable HTTP contract.
- A lifecycle service that publishes a durable change after commit must be the sole mutation notification boundary; handler-level reload-and-publish duplicates events and makes an already committed response fallible.
- A post-commit decode regression trigger must satisfy SQLite constraints while violating the Rust decoder, such as schema-valid JSON containing a denied unknown field.

## 2026-09-03 Task 15

- Calling a captured browser-native `fetch` as an object method changes its receiver; Chromium throws `Illegal invocation` before Playwright or the server can observe a request. Preserve the browser global receiver when injecting fetch into Ky.
- Responsive `display: none` on a button label removes its accessible name when the remaining icon is `aria-hidden`; icon-only breakpoints need an explicit state-aware label.
- Browser geometry captured during a transform entrance animation can transiently exceed the viewport by one pixel. Waiting on `document.getAnimations().map(animation => animation.finished)` produces stable scroll-ownership evidence without arbitrary sleeps.
- Explicit operational state must survive every responsive breakpoint. On narrow shells, hide the secondary technical endpoint label rather than the complete connection-status primitive.
- A finite mocked SSE response naturally moves from connected to reconnecting after EOF. Browser assertions should require a visible explicit lifecycle surface instead of pinning a timing-sensitive single state.
- Visual evidence directories are acceptance inputs: remove stale extra captures and verify exact file count and modification time after the final rendered-source edit.
- Plan ownership must be read from the exact downstream task definitions: Task 16 owns the Tasks table, Task 17 owns manual task actions, and Task 18 jointly owns Workers and Settings.
- Deterministic visual evidence should encode its rendering contract in Playwright: fixed CSS-pixel viewports, an explicit color scheme, reduced motion, settled animations, and `fullPage: false` prevent stale or oversized acceptance artifacts.
- Programmatic focus is only visually provable when the focused error primitive has an explicit `:focus` treatment; relying solely on browser `:focus-visible` heuristics leaves screenshot evidence ambiguous.
- A no-gradient operational surface can retain depth through solid luminance stacking, accent edges, inset highlights, and structural borders without flattening the shell.

## 2026-09-03 Task 15 Focus Convergence

- React passive effects can race an async DOM query under full-suite scheduling: the requested route was committed while `document.activeElement` remained `body`. Route-main focus belongs in `useLayoutEffect` so the accessibility transition completes before the committed shell is observable.
- Browser reload coverage should assert focus ownership after an existing session is restored; visible route content alone does not prove the keyboard-navigation contract.
