# Submission retry verification follow-up (2026-09-10)

The initial retry commit was pushed before full controller regression passed. Focused acceptance results did not establish that unrelated failures were environmental. This follow-up used the pre-change commit 65a37af and retry commit d192c93 for direct comparison.

## Observed evidence

- The unchanged baseline production-password test reproduced Database(PoolTimedOut), including once in five ordinary /tmp runs and in two separate syscall-traced runs.
- A timed strace of the baseline showed its SQLite thread entering fsync at 1789023115.088289; the test thread exited about five seconds later, and the unfinished syscall was interrupted during process exit at 1789023121.963947. The call did not return normally. This directly demonstrates storage synchronization stalling beyond the pool's five-second acquisition timeout in that run. A separate small-file fsync probe was fast and would not have established the cause alone.
- Baseline task20 on ordinary disk: 20 passed, 13 failed. Failures included PoolTimedOut, transfer checkpoint waits, and downstream completion waits. Thus those failure categories also occur without submission retries.
- A saved d192c93 task20 executable, verified to list 37 tests including submission_retry, passed 37/37 with TMPDIR=/dev/shm and four test threads. All submission, cancellation, crash recovery, outage, and worker-health tests were retained.
- Do not run the entire suite with TMPDIR=/dev/shm: cross-filesystem tests explicitly use /dev/shm as the second mount and would lose their different-device precondition. Process shutdown fixtures also create their databases under current_dir, ignoring TMPDIR, and can retain the original disk timeout.
- Sharing one Cargo target directory between worktrees produced a stale 33-test baseline task20 executable even after a build from the current checkout. That mixed-artifact full run was discarded. Rebuild the controller and transport packages cleanly before relying on full-suite results; use separate target directories for future revision comparisons.

No production task, worker, database, timeout, or durability setting was changed during these diagnostics. Test workers and their small synthetic video byte sequences are isolated fixtures.

## Actual assertion mismatch and correction

A clean rebuild of d192c93 with TMPDIR=/run/user/1008 completed with 581 passed, one failed, and one intentionally ignored test. The sole failure was the three-worker proof expecting one Run HTTP request and observing two. Its held acceptance response can outlive the request timeout; automatic confirmation then correctly replays the same submission. This was an outdated request-count assertion exposed by the feature, not evidence of a duplicate job, and should have been resolved before the initial push.

The three-worker scenario now explicitly injects AcceptThenDropRunResponse once per isolated mock worker and holds the first poll response to inspect all occupied compute slots. Each worker must receive exactly two Run requests, with identical nonempty idempotency keys and identical bodies, while pipeline proof still requires one remote job, one attempt, completed output, and released capacity. This makes recovery deterministic instead of relying on an incidental timeout while arranging tasks. The separate normal_attempt_submits_exactly_once assertion remains unchanged.

The corrected isolated test passed and reported run_requests=[2,2,2], remote_jobs=[1,1,1]. Strict all-target Clippy also passed, including the subsequent process-fixture change. No production implementation or timeout was changed in this follow-up.

## Reproduction commands

On this Linux host, /run/user/1008 and /dev/shm are writable, distinct tmpfs mounts. The second mount must stay distinct because cross-filesystem tests explicitly target /dev/shm. The process startup fixture now uses TempDir::new() so TMPDIR also selects its isolated database storage. The child still runs inside that fixture directory. This removes accidental source-disk dependence without changing startup, configuration-error, SIGTERM, or drain-bound assertions. Both tests passed in 0.68 seconds after this correction; previously they could also report "Controller listener did not open" when startup remained blocked.

```bash
TMPDIR=/run/user/1008 cargo test -p videnoa-controller --no-fail-fast -- --test-threads=4
cargo clippy -p videnoa-controller --all-targets -- -D warnings
```

Expected: all ordinary controller tests pass, with only the pre-existing opt-in authentication stress test ignored. This storage setup exercises real SQLite and real filesystem operations but does not qualify physical-disk crash durability or resolve the host's intermittent fsync stalls.

## Final verified result

The complete controller command above exited successfully after both fixture corrections: 582 passed, zero failed, and one pre-existing intentionally ignored authentication stress test. The 37-test task20 target passed in 205.91 seconds. Strict all-target Clippy, rustfmt checks of both modified Rust files, and git diff whitespace checks also passed. These results come from one complete current-checkout run, not a combination of partial reruns.
