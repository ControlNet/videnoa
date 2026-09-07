# Iroh development session handoff

## Latest rebase before feature implementation

Rebased onto local dev `ae801df` after the user requested the latest development
baseline. New feature HEAD is `604f441`. Rebase had no conflicts; range-diff
confirmed the sole feature documentation patch was unchanged. All three untracked
iroh knowledge files were backed up in `/tmp/videnoa-iroh-rebase-pil3qzaj` and
verified byte-for-byte before this record update. No application source was
changed, no runtime tests were run, and no remote push was performed. Ancestry
and the tracked diff against dev were verified. Earlier baselines below are
historical.

## Rebase onto completed worker authentication

On the user's explicit request, rebased `feature/iroh` onto local `dev` at
`7754537` (worker bearer verification and authenticated retry corrections).
Rebase completed without conflicts. `git range-diff 12af8b7..dace571
7754537..HEAD` confirmed the sole feature documentation commit was preserved
without patch changes. The two untracked handoff/registration knowledge files
were backed up outside the checkout and verified byte-for-byte after rebase.
No remote push was performed. No runtime tests were run for this documentation-
only replay; ancestry and patch equivalence were checked. The previous baseline
below describes the initial handoff, not the post-rebase branch.

## Sources and confirmed decisions

Reviewed the user messages in local Codex session
`01a0770a-f1d8-72a0-a227-186f07654637` and the existing iroh knowledge notes.

- Develop on `feature/iroh` in `/home/zhixi/GitRepos/videnoa-iroh`.
- The current Controller release is awaiting real-world validation. Iroh is
  intended for the subsequent release, whose version is not assigned.
- Start with TCP forwarding using the iroh ecosystem, specifically evaluating
  `iroh-proxy-utils`, while retaining Controller/worker HTTP contracts.
- Rust was upgraded to 1.98.0, including CI and Docker alignment. The user
  explicitly deferred other dependency upgrades until needed.

## Checkout verification

At handoff, HEAD was `dace571`, the working tree was clean, and `rustc --version`
reported 1.98.0. Root manifests contained no iroh dependency. This handoff adds
documentation only; no implementation or runtime validation was performed.
There is no checkout-local AGENTS.md; follow the instructions supplied in the
current conversation. Historical references to runtime environments in another
worktree do not establish that its untracked runtime files exist here.

## Read next

- [Feature worktree and release boundary](iroh-feature-worktree-2026-09-07.md)
- [Transport research](iroh-network-feasibility-2026-09-07.md)
- [Dependency experiment](iroh-dependency-readiness-2026-09-07.md)
- [Completed toolchain alignment](rust-stable-upgrade-2026-09-07.md)

The network research's original Rust 1.83/toolchain blocker is historical;
the subsequent toolchain alignment supersedes it. The separate-process tunnel
remains a prototype option, but is no longer required merely to isolate an old
Rust compiler. Embedded versus separately supervised forwarding remains an
implementation choice, not a recorded user decision.

## Suggested first implementation milestone

Build a small, real TCP forwarding path with a persistent identity at each end,
an approved Controller identity on the worker side, and a fixed allowed worker
API target. Connect the existing Controller HTTP client through a local listener.
Keep the prototype isolated from current release runtime state.

Before integrating broadly, verify HTTP health/catalog and streaming transfers,
then task submission, polling, cancellation, and cleanup. Add rejection tests
for unauthorized peers and targets. TCP forwarding cannot filter HTTP routes;
choose the worker API exposure boundary explicitly.

The prior temporary-copy experiment compiled the real Controller/core libraries
with iroh 1.1.0 and proxy-utils 0.3.0. It did not validate proxy API integration,
endpoint startup, NAT traversal, relay performance, Windows, or full regressions.
Recheck version-specific APIs when implementing. Update only necessary lockfile
entries, and scope the existing SQLx dependency exclusion test correctly when
iroh introduces legitimate identity cryptography dependencies.

Real-network acceptance still requires separate NAT/CGNAT networks, forced relay
fallback, identity persistence across restarts, interrupted transfers, and
concurrent bulk/control traffic. Existing file transfer behavior has no byte
offset resume; a tunnel alone does not add it.

## Handoff verification

```bash
git status --short --branch
rustc --version
git diff --check
```

Expected: branch `feature/iroh`, compiler 1.98.0, and no whitespace errors.
After this handoff, this knowledge file is the only added working-tree file.
