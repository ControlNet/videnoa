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

## 2026-09-03 Task 17

- A creation retry key belongs to one canonical request body, not to the dialog session. Clearing ambiguous intent on the first field edit makes changed-body key reuse unrepresentable in the UI.
- Task-list SSE payloads are useful invalidation evidence but are not authoritative detail. Refetching the selected task preserves persisted attempt history and current optimistic versions.
- Browser error fixtures must satisfy the same strict Zod contract as production, including the operations error code and each field error's machine code; otherwise the correct client behavior is `malformed_response`.
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

## 2026-09-03 Task 16

- A large task-history surface stays bounded when URL state is parsed into a closed query model and every trigger issues exactly one limited page request plus bounded aggregate counts.
- SSE row replacement requires both monotonic versioning and stable filter/order membership; an unseen update should invalidate only when it could enter the current query, not for every unrelated task delta.
- Narrow responsive ownership is clearest when filters reflow without overflow and only the table frame scrolls horizontally, with a bounded vertical viewport keeping the horizontal scrollbar reachable.
- Error and loading states must be independent: an initial failed page request should render the retryable alert without leaving a misleading skeleton table behind.
- Deterministic post-interaction screenshots should reload the final URL before capture because a focused pagination control can restore a previously scrolled viewport after programmatic scroll resets.

## 2026-09-03 Task 16 Remediation

- Empty-page recovery is both bounded and loop-safe when the response `limit` and `offset` agree with the current URL, and correction applies `floor((total - 1) / limit) * limit` only when that canonical offset is lower than the requested offset.
- Query-keyed page state prevents a failed changed-query request from rendering rows loaded for another URL, while clearing the keyed page at each request generation also keeps same-query retries from exposing stale results.
- SSE handling needs three outcomes rather than treating every non-merge as invalidation: stale/equal versions are ignored, stable active progress updates merge, and membership or ordering changes refetch one current page plus one count set.
- Evidence capture must set color/reduced-motion media before scrolling; changing media during capture can reset scroll state. Screenshot helpers should receive the complete filename once to avoid stale `.png` and fresh `.png.png` duplicates.
- A full desktop page exposed that the original table height budget pushed pagination outside the non-scrolling shell. Reducing the table-owned vertical viewport keeps dense scrolling and pagination simultaneously reachable.
- Narrow horizontal overflow is discoverable and keyboard-operable without making a non-interactive container focusable: expose a named `region`, pair it with visible Left/Right buttons, and connect a concise hint through `aria-describedby`.
- For oversized table columns, `offsetLeft` is not a reliable scroll-frame coordinate. Align a header by adding its viewport-space left delta to the current `scrollLeft`, clamped to the frame's scroll range.
- Evidence should align an oversized diagnostic column by its leading edge rather than the absolute scroll end; otherwise the right edge is visible while the header label and ellipsis can remain off-screen.

## 2026-09-03 Task 16 Final Remount

- A retained external-store snapshot is historical state for a newly mounted subscriber. Initializing the applied generation from the store snapshot at hook mount suppresses replay while preserving updates published after that render because the ref retains the earlier baseline.
- Pagination ranges should require a non-empty current page whose response offset and limit match the URL and whose offset is below total. Empty or contradictory pages can still report the server total while truthfully displaying `0-0`.

## 2026-09-03 Task 17 Oracle Remediation

- Frontend retry visibility must mirror `Lifecycle::retry_mode` as an allowlist of exact failure-code/stage pairs; `retryable=true` is necessary but cannot authorize contradictory persisted evidence.
- Cancellation status alone is insufficient once durable intent exists. Authoritative `cancel_requested_at` must be null before the frontend exposes the first cancellation request.
- Intake recovery guidance must inspect structured field, closed field-error code, and message because real Rust validation deliberately keeps the top-level message generic.
- Programmatic safe focus after a pointer-opened confirmation does not necessarily match `:focus-visible`; a confirmation-specific `:focus` treatment is needed for visible evidence.
- Scroll-contained detail evidence is truthful when fixed-viewport captures enumerate the top General/Progress state and the lower Attempts/Error state rather than attempting a clipped full-content element screenshot.

## 2026-09-03 Task 17 Evidence Convergence

- Replay evidence represents one logical task only when the authoritative response fixture explicitly matches every submitted field; helper defaults can otherwise produce a visually contradictory success capture despite correct request assertions.

## 2026-09-03 Task 18

- Operational tables need explicit left, middle, and right evidence at narrow widths; page-level no-overflow assertions alone do not prove every locally scrolled column and action is reachable.
- Element screenshots are the reliable evidence shape for a complete section inside a fixed-height shell, while page-context captures establish navigation and responsive hierarchy.
- Mutation fixtures should parse submitted bodies through the same production Zod schemas and journal optimistic versions so the browser test proves both visible behavior and exact API contract use.
- Readiness rows need a small inline-end inset because edge-aligned flex text can lose antialiased pixels at element screenshot boundaries even when the page itself does not overflow.
- Operator guidance must key from the exact serialized `OperationsError` messages, not the internal `WorkerRegistryError` display strings that are translated before HTTP; production-shaped fixtures and a shared mapping test prevent raw error leakage.
- A conflict refetch is incomplete while an edit dialog retains the original record object. Derive the submitted worker version from the refreshed list while keeping form fields local, so a reviewed retry advances without erasing operator input.

## 2026-09-03 Task 18 Remediation

- Restoring row-action focus before a mutation settles is racy because the temporary disabled state makes the browser discard focus. Cancellation can restore immediately, while confirmed deletion must restore only after mutation settlement and the enabled button render.
- A worker deletion confirmation needs the exact row trigger, not a page-level fallback: passing `event.currentTarget` through the table action preserves precise focus restoration across Escape, safe cancellation, and rejected deletion.
- Scheduler pause guidance must distinguish admission from continuation: no new reservation, prefetch, or compute starts, while active processing, applicable transfer/publication, and cleanup continue.
- A production-shaped authentication failure is the top-level `401 {"error":"unauthorized"}` boundary, unlike nested OperationsError responses; browser coverage should prove it replaces the operational shell without rendering request proof or credential material.
- Contained horizontal overflow is not keyboard-accessible merely because row actions are tabbable; the overflow owner itself needs a labelled focus stop so keyboard users can pan hidden columns before reaching controls.

## 2026-09-03 Task 18 Accessibility Convergence

- Post-confirm focus must wait for React to commit mutation teardown because the invoking row action remains disabled until `mutating` clears; a render-synchronized generation closes the browser timing gap.
- Delete outcomes need different stable targets: cancellation and rejection return to the exact row action, while successful removal focuses `Add Worker` because the original trigger no longer exists.
- Native numeric constraints alone do not create associated inline error messages. `noValidate` lets the existing Zod boundary own deterministic first-invalid focus, stable error IDs, `aria-describedby`, and alert announcements while retaining min/max metadata.
- Server field errors need the same mapping and focus path as local Zod errors, including `compute_slots`; adjacent rendering without focus is not equivalent accessibility.

## 2026-09-03 Task 19

- Route-level axe scans exposed light-theme semantic-token contrast failures that component-focused tests did not cover; correcting the shared quiet, accent, and healthy tokens fixed every affected operational surface together.
- A controlled number input initialized in a passive open effect can merge a fast browser fill with the prior value (`4` plus `1` became `41`). Mounting the dialog only while open and initializing fields synchronously from the selected worker removes that observable race while refreshed optimistic versions continue to flow through props.
- Table-owned horizontal overflow is discoverable when the named region is paired with visible narrow-only Left/Right controls and a concise `aria-describedby` hint; focusability alone does not explain the hidden columns.
- Final browser evidence is reliable when all historical scenario screenshots, the HTML report, and result metadata are routed under one task directory, traces/videos are disabled, and signatures plus modification times are checked after the last rendered-source edit.

## 2026-09-03 Task 19 Session Remediation

- A session-expiry browser test is vacuous unless it first seeds and observes the named HttpOnly cookie. Modeling the production expired `Set-Cookie` response then proves browser cookie removal rather than only proving storage was already empty.
- Invalid `/api/auth/session` responses should preserve the typed unauthorized body and status while expiring the existing cookie through the same cookie builder used by logout.
- Duplicate screenshot basenames are not stale evidence when they belong to path-scoped historical matrices; comparing full paths and content hashes distinguishes intentional route captures from duplicate files.

## 2026-09-04 Task 23

- Deterministic GNU tar/gzip output requires normalizing member order, ustar metadata, owner/group, modes, timestamps, and the gzip header; two independently produced Controller archives then share one SHA-256.
- Exact ordered-manifest comparison is a stronger archive boundary than a forbidden-name list because every unapproved loose asset, runtime, model, cache, secret, or existing product binary fails closed.
- Release archive smoke can prove frontend embedding by starting only the extracted binary with external config/data/hash paths and successfully requesting both `/api/health` and `/` without a sibling frontend directory.

## 2026-09-04 Task 22

- A separate Node stage plus `VIDENOA_CONTROLLER_WEB_PREBUILT=1` lets the container build frontend assets once, while the release Rust build still verifies and embeds `dist/index.html` through `rust-embed` without carrying Node or loose assets into runtime.
- BuildKit registry, git, npm, and target cache mounts provide isolated dependency/frontend caching without generated placeholder sources or changes to the existing GPU Dockerfile.
- A numeric non-root runtime requires bind-mounted admin hash files to be readable by UID 10001; read-only mount semantics protect the file, while host ownership/mode must permit the container user to read it.
- Exported filesystem, package, and `ldd` inspection are complementary GPU-free checks: package names alone do not prove that no model or runtime artifact was copied into an otherwise CPU base.

## 2026-09-04 04:33:03 +10:00 Task 24

- CI/release preservation is best locked by parsing the actual YAML, validating the dependency DAG, and asserting complete legacy plus Controller job/asset/tag contracts; token-only greps cannot prove failure propagation or non-overlap.
- The dedicated archive and container scripts already provide the strongest negative boundaries. Workflows should call them directly so wrong versions, missing files, extra GPU/model content, and invalid runtime images fail at the same reusable contract locally and on hosted runners.
- A release is complete only when the final GitHub release job depends on every legacy and Controller archive/image publication and post-publication verification checks all four Docker tags and all archive products.

## 2026-09-04 Task 25

- Archive documentation must contain its own first-run, persistence, ambiguity,
  backup/restore, upgrade, and rollback path because release archives contain no
  `docs/` directory.
- A source-accurate config smoke can replace example paths with isolated
  temporary directories and generate an ephemeral Argon2id hash, then load the
  result through `ControllerConfig` without persisting test credentials.
- SSE documentation is correct only when it distinguishes the initial and
  lagged `refetch` signal from durable SQLite/API truth and keeps attempt history
  on server-paginated HTTP responses.

## 2026-09-04 F4-B3 Legacy Linux Package Remediation

- p7zip 16.02 exit code 2 is a fatal operation error, while bad command-line parameters use exit 7. The hosted log reached input scanning and archive creation before `System ERROR: E_FAIL`, so argument order, quoting, cwd, missing input, and initial output-path resolution were not the cause.
- The legacy bundle is about 5.33 GB but compresses to about 2.29 GB. GitHub-hosted Linux guarantees only 14 GB free, so post-build cache reclamation plus a conservative full-input-size disk preflight avoids image-dependent archive headroom failures.
- Split archive verification must enter through `.7z.001`, while the same helper can preserve the existing unsplit `.7z` fallback. `7z t` proves integrity before `7z l` checks the `videnoa/` root.

## 2026-09-04 F4-B2 Auth Focus Remediation

- Recoverable async focus belongs in `useLayoutEffect` after React commits the current error generation; passive focus can remain on the password field under hosted scheduling.
- A monotonically increasing submit generation prevents stale or post-unmount login responses from rendering or stealing focus while still refocusing repeated identical failures.
- Authentication semantics must distinguish invalid credentials from transport or malformed-response recovery so only credential rejection marks and describes the password input as invalid.
- Full-shell jsdom tests need a local `ResizeObserver` boundary after the task table adopted measured overflow; mirroring the task component test stub keeps browser-only API compatibility out of production code.

## 2026-09-04 07:10:53 +10:00 F1-B1/F3-B1 Worker Health Remediation

- Worker creation is only durable registration; a production-owned runtime must probe `/api/health`, discover capabilities, and persist the online transition before scheduling can select the worker.
- Persisting health through `WorkerRegistry::refresh_health` emits the existing durable worker change, so capability discovery wakes queued scheduling without a separate in-memory notification path.
- Failed probes should retain durable capabilities and last-seen evidence while marking the worker offline and advancing bounded backoff; successful probes replace eligible workflows and reset retry state.
- Production-shaped tests must register workers through the authenticated API and wait for the runtime transition. Failure-path tests need a no-wait registration helper so they can observe the first offline result without injecting database state.

## 2026-09-04 F3-B2 Task Overflow Remediation

- Overflow affordance visibility and scroll-boundary disabling require different measurements: compare the rendered table `offsetWidth` to the frame `clientWidth` for actual content overflow, but use the frame scroll extent plus reserved scrollbar gutter for effective left/right boundaries.
- Chromium with `scrollbar-gutter: stable` can stop about the gutter width before `scrollWidth - clientWidth`; treating that difference as content-overflow tolerance hides real small overflows, while treating it only as edge tolerance preserves both discoverability and correct disabled states.
- A deterministic one-pixel overflow unit case prevents the reserved gutter from suppressing navigation, while production-preview browser coverage remains responsible for the actual effective scroll-end geometry.
- Fresh fixed-viewport captures at 1440, 1024, and 375 pixels plus independent visual passes distinguish expected table-viewport clipping from document overflow, cell escape, or CJK glyph defects.

## 2026-09-04 Rust 1.83 Clippy Compatibility

- Clippy lint-group membership changes across toolchains: Rust 1.83 includes `module_name_repetitions` in `pedantic`, while the current toolchain does not enable it through the same group. A named package-level override can preserve stable strict-group policy without renaming public APIs.
- Resolving the initial 95 repeated-module-name and 48 must-use diagnostics exposed five previously masked `let_and_return` findings in integration tests; cross-version lint remediation should iterate until the old-toolchain command is fully clean.
- `PathCapabilities::open` requires every configured root to exist. Test fixtures must create `data_root` and `temp_root` as well as input/output roots before opening capabilities.

## 2026-09-04 Rust 1.83 Clippy Compatibility Correction

- A package-wide `module_name_repetitions` allowance is not an acceptable compatibility mechanism because it hides real diagnostics across the package. The accepted solution assigns responsibility-based logical names to implementation modules and retains established public paths through root façade modules.
- Rust `#[path]` changes a module's logical name without moving the source file. This resolves repeated module/type naming while leaving Rust item names and serde, SQLx, Clap, HTTP, and database string contracts untouched.
- Integration support files directly under `tests/` become standalone Cargo test targets. Shared fixture code must live below the owning test module, as in `tests/task_api/support.rs`, to avoid accidental public-target lint requirements.

## 2026-09-04 Rust 1.83 Clippy LOC Completion

- A complete NUL-safe changed-file audit is required after targeted size fixes; it found `mock_videnoa/recovery_support.rs` at 323 pure LOC, `task12/support.rs` at 360, and an additional `main.rs` boundary at 251 that earlier partial reports missed.
- Test helpers loaded through `#[path]` need explicit child paths when their extracted sources live below a directory named after the parent file. This keeps nested support private without creating standalone integration targets.
- Re-exporting test utilities can become an unused import when the shared parent is compiled into several integration targets. Thin root delegates preserve existing helper paths and satisfy strict Clippy without allowances.

## 2026-09-04 Final Wave F2 Task Overflow Determinism

- A right edge derived from scroll geometry can become stale when fonts or table layout expand after keyboard `End`; preserving the prior effective-edge intent during layout observers keeps the control state stable without blocking ordinary user scroll events.
- Native disabled state and computed unavailable styling must be captured in one browser evaluation. Sequential locator assertions can truthfully observe different render generations even when each individual read is correct.
- Deterministic layout-growth fixtures are stronger than timing amplification: increasing the rendered table width after reaching the edge reproduced the race on every run without sleeps or retries.

## 2026-09-04 F3 SSE Shutdown Remediation

- Axum graceful shutdown stops listener intake but still waits for response bodies; every long-lived SSE stream must independently observe the owning runtime cancellation token.
- Deriving HTTP shutdown and per-stream child tokens from the existing `ShutdownCoordinator` preserves one cancellation tree without changing durable stage/write drain or remote compute semantics.
- An external authenticated SSE regression must prove both listener closure and process exit while the client remains connected; either observation alone misses the original hang.

## 2026-09-04 F4 Duplicate Submission Remediation

- Remote idempotency prevents duplicate jobs but does not prevent duplicate requests; uncertain acceptance needs a separate durable executor-ownership boundary.
- A Reconciler-generation owner persisted with attempt-version CAS blocks same-process replay while allowing a newly constructed Controller generation to take over and replay the unchanged key after restart.
- Submission ownership must be claimed after scheduler admission and immediately before the remote request. Claiming before admission strands paused work under the current generation.
- Cancellation of a `Submitting` attempt cannot bypass uncertain-acceptance ownership. Reusing the same claim boundary proves the owning generation emits neither a second `/api/run` nor a cancellation request without durable remote evidence.
- Migration upgrade coverage must construct a database at the prior migration boundary; reopening a fully current database proves idempotence but not that the new column is added to deployed state.

## 2026-09-04 F3 SSE Hosted Startup Remediation

- Debug Controller process tests require `controller-web/dist` at runtime, while `build.rs` intentionally creates or validates that directory only for release builds. A clean hosted debug-test job must therefore build Controller web assets in the same job before launching the binary.
- Child stderr is part of a process-test failure contract. Piping and reporting it on early exit distinguished the hosted `FrontendDirectoryMissing` startup error from bind and configuration failures without changing startup or shutdown timing.
- Removing only `controller-web/dist` reproduced the hosted status-1 failure; restoring the unchanged directory toggled the authenticated SSE test back to listener closure at 0 ms and process exit between 8 and 21 ms across repeated Rust 1.83 runs.
