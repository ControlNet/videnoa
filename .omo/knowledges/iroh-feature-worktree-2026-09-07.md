# Iroh feature branch and release boundary

## User decision

The current Controller will undergo real-world validation before the next
release. Iroh belongs to the following release and must be developed in its own
feature worktree. Do not merge iroh work into the current release branch as part
of setup. No version number has been assigned to the future release.

## Workspace layout

- Current release worktree: `/home/zhixi/GitRepos/videnoa`, branch `dev`.
- Iroh worktree: `/home/zhixi/GitRepos/videnoa-iroh`, branch `feature/iroh`.
- Branch base: `12af8b7`, the current synchronized `dev` commit at creation.
- Rust baseline: 1.98.0. Existing dependency upgrades remain deferred until
  required by feature implementation; do not import the investigation lockfile.
- First-phase direction: TCP forwarding using `iroh-proxy-utils`, preserving
  existing Controller/worker HTTP contracts.

Setup creates the worktree and records the release boundary only. It does not
implement iroh, change dependencies, migrate databases, or run application
services. Git worktrees share the repository and refs, but maintain separate
checked-out files and indexes. Untracked model files, native runtime libraries,
environment files, and installed frontend dependencies are not copied by Git.
Provision feature runtime state separately before running services; do not point
feature experiments at the current Controller's production state.

## Continue work

```bash
cd /home/zhixi/GitRepos/videnoa-iroh
git status --short --branch
rustc --version
```

Expected branch: `feature/iroh`; compiler: Rust 1.98.0.
Start with `iroh-dependency-readiness-2026-09-07.md` and
`iroh-network-feasibility-2026-09-07.md` in this knowledge directory.
