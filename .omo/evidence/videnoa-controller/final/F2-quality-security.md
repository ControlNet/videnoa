# F2 Code Quality, Security, and Data-Integrity Review

Audit date: 2026-09-04

Audited tip: `22f9656b797f5c3326ff003d11d812a6d212ed8b`

Repository baseline: `HEAD == origin/dev`, divergence `0 0`, clean before verification began.

## Verdict

**REJECT**

The two blockers from the prior F2 review are resolved: the exact Rust 1.83 strict Clippy gate now passes, and Controller temporary workspaces use retained capabilities with no-follow opens, identity checks, regular-file checks, descriptor-relative operations, and hostile replacement tests. However, the required Controller web Playwright gate is not reliable at the audited tip. The exact full suite failed 1 of 45 tests, and the failing scenario reproduced in 1 of 3 isolated repetitions. A release gate that intermittently fails on an asserted user-visible control state remains a material code-quality blocker.

## Blocking Finding

### F2-B1: Task overflow browser verification is flaky

- `npm run test:e2e` failed `tests/e2e/task-overflow.spec.ts` while 44 other Chromium scenarios passed.
- The failure occurs after horizontal navigation reaches the right edge. `expectUnavailableControlStyle(right, left)` observes both controls with computed opacity `"1"`, although the unavailable control must differ from the available control.
- `npx playwright test tests/e2e/task-overflow.spec.ts --repeat-each=3` reproduced the same assertion failure once and passed twice.
- The helper first verifies that the unavailable control is disabled and the peer is enabled, then reads their computed styles. The intermittent result therefore exposes unstable rendered state or an unstable assertion boundary rather than a deterministic unsupported-browser condition.

Required correction: make the scroll-edge state and its browser assertion deterministic, then demonstrate that the exact full Playwright suite and a repeated focused run pass without retries.

## Resolved Prior Blockers

### Rust 1.83 Clippy compatibility

- `cargo +1.83.0 clippy -p videnoa-controller --all-targets --all-features -- -D warnings`: PASS.
- Current-toolchain `cargo clippy -p videnoa-controller --all-targets --all-features -- -D warnings`: PASS.
- The responsibility-named private module topology and narrow lint policy compile cleanly under the CI-pinned toolchain.

### Temporary workspace path safety

- `TempWorkspace` retains the configured root capability, opened task-directory handle, directory identity, relative leaf, and display-only path.
- Re-entry calls verify the configured root and task-directory identity before returning a cloned retained handle.
- Artifact reads and create/truncate operations are descriptor-relative, no-follow, nonblocking, and restricted to regular files.
- Cleanup reopens the task directory without following links, checks identity, removes relative to the retained root, and synchronizes the retained root directory.
- Task 12 attacks prove task-directory and artifact-leaf symlinks cannot modify outside sentinels and FIFOs fail without blocking.
- Task 13 attacks prove configured-root and task-directory replacement cannot delete outside the owned tree.

## Security and Integrity Dispositions

| Area | Result | Basis |
|---|---|---|
| Authentication and authorization | PASS | Operational routes require session or explicit Bearer authentication. Cookie mutations require same-origin and CSRF proof. Failed-login throttling and password-hash rotation tests pass. |
| Sessions and secrets | PASS | Argon2id password verification, digest-only session persistence, cookie expiry, rotation invalidation, and redaction contracts pass. The tracked scanner hits are synthetic negative/redaction fixtures, not credentials. |
| CSRF and CORS | PASS | Cross-origin preflight receives neither an allowed-origin nor credentials header; cookie mutations reject missing same-origin/CSRF proof. |
| SQL, migrations, indexes, and CAS | PASS | Fresh, idempotent, and rollback migration tests pass. Bound queries, uniqueness/enum constraints, optimistic versions, atomic reservations, snapshot pagination, and the 20,000-row query-plan suite pass. |
| Lifecycle and recovery | PASS | Exhaustive lifecycle tables, cancellation boundaries, bounded retry, durable transition ordering, restart matrices, malformed-task isolation, and no blind resubmission contracts pass. |
| Input and temporary paths | PASS | Root identity, no-follow input reopen, traversal/symlink/FIFO rejection, temporary capability replacement attacks, and owned cleanup tests pass. |
| Publication and no-clobber | PASS on Linux | Descriptor-rooted staging, regular-file checks, final hashing, racing destination preservation, duplicate finalizers, cross-filesystem copy, and no-overwrite finalization pass. Native Windows race behavior remains a host-boundary risk. |
| Panic, unsafe, and typed errors | PASS | `unsafe_code` is forbidden. The only production-tree `panic!` search match is inside an `asset_path.rs` test module; no production `unwrap`, `expect`, `todo`, or `unimplemented` path was found. |
| Module size | PASS | NUL-safe tracked-file inventory found a maximum of 248 nonblank/non-line-comment lines in Controller production modules; no file exceeds the 250 pure-LOC ceiling. |
| Dependency isolation | PASS | Controller's normal/build dependency tree has no `videnoa-core`, ONNX Runtime, CUDA, cuDNN, TensorRT, or model-runtime match. Frontend install reported zero vulnerabilities. |
| Packaging and docs content | PASS | Linux archive, Windows static archive, root-file, legacy archive-helper, workflow-contract, and Controller documentation checks pass. |

## Verification Performed

- Baseline: `HEAD == origin/dev == 22f9656b797f5c3326ff003d11d812a6d212ed8b`, divergence `0 0`.
- `cargo fmt --all -- --check`: PASS.
- Rust 1.83 and current strict Controller Clippy: PASS.
- `cargo +1.83.0 test -p videnoa-controller --all-targets`: PASS.
- Focused Task 20 crash/outage suite: 26/26 PASS.
- Focused Task 21 load/concurrency/filesystem/resource/security suites: 94/94 PASS.
- `cargo +1.83.0 doc -p videnoa-controller --no-deps`: PASS.
- `npm ci --no-fund`: PASS, 0 vulnerabilities.
- `npm run lint`: PASS.
- `npm test -- --run`: PASS, 108/108.
- `npm run typecheck`: PASS.
- `npm run build`: PASS.
- `npm run test:e2e`: **FAIL**, 44/45 passed; material blocker F2-B1.
- Focused overflow repetition: **FAIL**, 2/3 passed; reproduced F2-B1.
- `bash scripts/tests/controller_docs_test.sh`: PASS.
- Controller archive and workflow contract scripts: PASS.
- Tracked secret scan: two reviewed synthetic fixtures; no usable credential or private key found.
- Gitignore audit: `.env` is covered; generic key/credential filename patterns remain uncovered as a hardening gap.
- `git diff --check ca0b27e..HEAD` and working-tree `git diff --check`: PASS.
- Rust and TypeScript LSP diagnostics: 0 diagnostics in the scanned Controller source trees.

## Audit Boundary

The audited change range contains 97 implementation, test, workflow, and script files with 3,210 additions and 1,210 deletions. Native Windows publication races and hosted release execution remain external-host boundaries. A concurrent, unrelated modification to `F1-plan-compliance.md` appeared during verification and was not changed or included in this review.

VERDICT: REJECT
