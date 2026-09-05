# Controller Task API test runtime

## Scope and measured cause

Baseline: `dev` at `f3bca6f`, Rust 1.83.0, Linux x86_64, Intel Xeon (Icelake),
60 visible CPUs. Measurements used the existing unoptimized test profile. The
first Cargo run compiled dependencies (23.95 s); it was excluded from all
comparisons. That run reproduced `Database(PoolTimedOut)` in the constrained
concurrency fixture. Both subsequent warm baseline suite commands passed.
This machine did not reproduce the reported many-minute absolute runtime.

The original six-test suite performed six synchronous production hashes while
building fixtures and 22 production-cost Bearer verifications: nine in concurrent
intake (eight POSTs plus one GET), five in create/replay/history/detail, two in
output validation, and six in rate limiting. The sixth invalid password is still
verified before the limiter returns 429. Anonymous and missing-peer requests do
not verify a password. Total: **28 production-cost Argon2 operations**.

A temporary, uncommitted Rust probe measured `hash_password()` at 558.6 ms and
five production verifications at 2.758 s (551.7 ms each). These costs account for
the serial runtime. The test-only PHC took 165.2 ms for 100 verifications (1.65 ms
each) through `Argon2::default().verify_password()`.

Each fixture owns a separate temporary SQLite database. Only the concurrency
fixture limits the pool to one connection and both acquisition/busy timeouts to
100 ms. Its old synchronous fixture hash blocked the current-thread Tokio runtime
after opening SQLite, delaying pool maintenance independently of production
`spawn_blocking` verification. After removing that fixture KDF and generic Bearer
load, a 30-run diagnostic still
failed 7 times while opening/migrating the constrained database, before login or
requests. Isolating the regression into `task_api_concurrency` reduced that to one
failure in 30. Completing schema creation before opening the constrained pool
then passed 30/30 combined target runs. Migration setup uses ordinary database
options; every login and task request uses the unchanged one-connection, 100 ms
pool. Fixture errors now identify database open, credential seed, scheduler load,
and login stages. No request-phase timeouts were increased.

## Test architecture and security boundaries

- `tests/task_api/support.rs` contains the deterministic test-only Argon2id PHC:
  v19, m=64 KiB, t=1, p=1, salt bytes `videnoa-test-salt`, for the existing public
  test password. It is explicitly forbidden for production use.
- Each fixture logs in once through `POST /api/auth/login`. `SessionClient` captures
  the real session cookie and CSRF response header. Functional requests reuse that
  cookie; mutations include matching Host, Origin, and CSRF headers.
- Normal `task_api` now performs **zero production-cost hashes/verifications**.
  Its six tests perform six low-cost login verifications and seven explicit
  low-cost Bearer verifications (six invalid attempts plus one successful mutation).
  The isolated ordinary concurrency target adds one low-cost login.
- Missing-peer, anonymous rejection, and invalid Bearer/rate-limit tests retain
  their explicit authentication paths. A new boundary test rejects session
  mutations missing CSRF or Origin and accepts Bearer without session/CSRF.
- `task_api_concurrency` is a separate, non-ignored ordinary target in
  `--all-targets`. Concurrent intake still releases eight equivalent POSTs at a
  barrier with one
  idempotency key, requires one 201 and seven 200s, and queries exactly one task.
  The one-connection pool and 100 ms request-phase limits are unchanged.
- `tests/auth_contention.rs` separately sends eight production-cost Bearer requests
  on a current-thread executor, retaining the constrained SQLite pool. It checks
  successful HTTP responses and a 10 ms heartbeat. The maximum allowed heartbeat
  gap is half the measured production hash time, floored at 100 ms for scheduling
  tolerance. It uses `hash_password()` before opening SQLite.
- The production-cost stress test is ignored in normal runs and explicitly run in
  `.github/workflows/unittest.yaml`'s `controller-fault-load` job. The existing CI
  workflow contract validator and its negative test enforce that explicit command.
- A temporary mutation replacing production `spawn_blocking` verification with
  inline verification made the stress test fail (HTTP 500). The production source
  was restored byte-for-byte immediately afterward; no mutation is committed.
- `tests/auth_password.rs` explicitly locks down production PHC generation
  (Argon2id v19, m=19456, t=2, p=1, salt, 32-byte output) and checks correct/wrong
  passwords through the real production authentication service. Existing setup,
  rotation, expiry, session, CSRF, and rate-limit targets are retained.
- No production source files, password semantics, scheduler behavior, Cargo
  dependencies, lockfile, or project rules are changed.

## Warm before/after measurements

Each phase ran the suite to warm the build before measuring the same commands.
Cases below ran individually with their full filter and `-- --exact`. No temporary
instrumentation or profile override was active during these measurements. The
after suite includes the additional auth boundary test. The original
concurrency case is timed separately in `task_api_concurrency`; a combined command
is also reported to make the preserved coverage cost visible.

| Command/case | Before test execution | After test execution | Before whole command | After whole command |
| --- | ---: | ---: | ---: | ---: |
| `task_api`, default parallel | 4.12 s | 0.18 s | 4.483 s | 0.523 s |
| `task_api -- --test-threads=1` | 13.38 s | 0.33 s | 13.739 s | 0.677 s |
| Rate-limit rejection | 3.98 s | 0.05 s | 4.332 s | 0.386 s |
| Concurrent duplicate intake (now isolated) | 3.44 s | 0.08 s | 3.802 s | 0.435 s |
| Create/replay/history/detail | 3.40 s | 0.05 s | 3.741 s | 0.393 s |
| Missing key/existing output | 1.72 s | 0.04 s | 2.055 s | 0.392 s |
| Missing peer metadata | 0.58 s | 0.06 s | 0.919 s | 0.400 s |
| Anonymous rejection | 0.59 s | 0.09 s | 0.926 s | 0.425 s |
| New session-CSRF/Bearer boundary | N/A | 0.05 s | N/A | 0.388 s |
| Both ordinary targets, default parallel | N/A | 0.19 + 0.12 s | N/A | 0.642 s |

The primary target's execution improved 22.9x parallel and 40.5x serial. Including
Cargo startup, command wall time improved 8.6x and 20.3x. Running both ordinary
targets together still takes only 0.642 s wall time (7.0x faster than the original
combined target's 4.483 s), so the gain does not depend on omitting concurrency
coverage. Immediately after the PHC/session refactor, while concurrency was still
in the same target, measured execution was 0.19 s parallel and 0.37 s serial; the
architectural speedup predates isolation of the remaining migration flake.

Libtest reports execution rounded to hundredths of a second; command timings use
Python stdlib `time.perf_counter()` around `subprocess.run()`. Cargo's warm
metadata/startup cost is now a substantial part of the development command.

## Reproduction and development loop

Warm once, then measure both modes without changing profiles or running competing
builds. `/usr/bin/time` includes Cargo overhead; libtest's final line reports the
execution time separately.

```bash
cargo +1.83.0 test --locked -p videnoa-controller --test task_api
/usr/bin/time -p cargo +1.83.0 test --locked -p videnoa-controller --test task_api
/usr/bin/time -p cargo +1.83.0 test --locked -p videnoa-controller --test task_api -- --test-threads=1
/usr/bin/time -p cargo +1.83.0 test --locked -p videnoa-controller --test task_api --test task_api_concurrency
/usr/bin/time -p cargo +1.83.0 test --locked -p videnoa-controller --test task_api_concurrency concurrency::concurrent_duplicate_intake_creates_exactly_one_task -- --exact
```

Use a specific filter during edits, then the default-parallel target. Before
completion run all targets plus the separately opted-in production-cost stress
regression. All commands should pass; ordinary `task_api` requires no global
serialization. Stress output reports KDF duration and heartbeat progress.

```bash
cargo +1.83.0 fmt --all -- --check
cargo +1.83.0 clippy --locked -p videnoa-controller --all-targets --all-features -- -D warnings
cargo +1.83.0 test --locked -p videnoa-controller --all-targets
cargo +1.83.0 test --locked -p videnoa-controller --test auth_contention -- --ignored --nocapture
node scripts/tests/validate_ci_release_workflows.test.mjs
```

Controller Web behavior is unchanged; Web tests are unnecessary for this patch.

## Cargo profile experiment

No profile override is retained. A narrow experiment used:

```bash
cargo +1.83.0 test --locked -p videnoa-controller --test task_api --config 'profile.test.package.argon2.opt-level=3'
```

After warming each variant, three parallel executions were 0.33/0.19/0.17 s with
the existing profile versus 0.29/0.20/0.21 s with optimized Argon2. Median command
wall time was 0.541 s versus 0.546 s. There is no material improvement; remaining
SQLite fixture and Cargo costs dominate. The initial override required 22.9 s of
command wall time including recompilation, versus roughly half a second warm.
That is an incremental rebuild observation, not a pristine cold-build benchmark.
A later source-only rebuild with the optimized dependency already cached took
7.1 s. Broad dependency optimization would impose additional cold-build work
without evidence of a useful inner-loop gain, so it was not added.

## Final validation

- `cargo +1.83.0 fmt --all -- --check`: passed.
- Strict Rust 1.83.0 Controller Clippy, locked, all targets and features: passed.
- Full locked Controller `--all-targets`, default libtest parallelism: 427 passed,
  zero failed, one intentionally ignored stress test across 49 test binaries.
  Summed test execution was 385.26 s, mostly existing fault/load coverage outside
  this optimization; compilation took 13.98 s separately.
- Both ordinary Task API targets: 30/30 repeated default-parallel commands passed
  after isolation and completing migration setup before the constrained pool.
- Explicit production-cost auth contention: passed twice. Final warm execution
  was **2.80 s**, whole command **3.162 s**. Production hash: 542.9 ms; eight Bearer
  requests: 2.202 s; 221 heartbeat ticks; maximum gap: 11.0 ms against a 271.4 ms
  calibrated bound. First warmup execution was 2.85 s (3.233 s whole command).
- Workflow contracts, including removal of the explicit stress CI step: passed.
- Staged secret scan: passed. Only explicitly public test credentials are added.
- Final diff leaves production source, Cargo profiles and lockfile unchanged.
