# task20 flakiness: fsync stalls on the dev VM (2026-10-02)

## Finding

The intermittent `task20` failures are not a code regression. They come from
the dev VM's root disk (`/dev/vda2`, ext4, 93% full, virtio disk reported as
rotational), where fsync of a 14-byte file sometimes stalls for seconds.

- Ten-minute fsync probe on `/tmp` (one fsync every 0.2 s): p50 14.7 ms,
  p99 1.5 s, max 11.5 s; 14 stalls over 5 s.
- Baseline `e559336` fails the same way as HEAD (`e586204`): plain-disk runs
  had head 3/3 pass and base 1/2 (`one_worker`, `after_delete timed out;
  status=Downloading`).
- An instrumented `one_worker` failure: all HTTP to the mock Worker finished
  at 13.362 s; the task moved `downloading -> verifying` at 28.458 s, with
  one download request and no retry. That gap holds only local work: writing
  the part file, `sync_all`, evidence, rename, and the SQLite commit. The
  mock checkpoint wait (`await_reached`, 5 s) expired meanwhile.
- With `TMPDIR=/dev/shm/...` (fixtures, SQLite and the flock live under
  `temp_dir()`), HEAD passed 6/6 in 195-197 s, against 228-348 s on disk.
- Earlier `Database(PoolTimedOut)` failures in other controller test
  binaries during full-workspace runs fit the same cause: a writer blocked
  in fsync holds SQLite while other connections wait.
- The Controller orchestrator advances each task in its own JoinSet stage, so
  one stalled task does not block others (checked in `recovery_scan.rs`).

## Running reliably on this machine

```bash
mkdir -p /dev/shm/videnoa-tests
TMPDIR=/dev/shm/videnoa-tests cargo test --locked -p videnoa-controller --test task20
```

CI runners (SSD) are not affected; dev CI has been green.

## Not done

Harness deadlines were not changed. The positive waits are hard-coded in
about 26 places across 18 files (5 s mock checkpoints, 10-15 s proofs), and
some nearby windows assert that something does *not* happen, so they need
separating before any increase.
