---
id: BR-6
title: Reuse one rclone process per sync instead of re-authenticating per call
status: backlog
priority: none
assignee: jpsyx
labels: [enhancement, performance]
estimate:
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-10
updated: 2026-08-10
---

# BR-6: Reuse one rclone process per sync instead of re-authenticating per call

## Description

Every remote operation in a sync spawns a fresh `rclone` process, and each one
re-authenticates with the provider before doing any work. Measured against a
real Backblaze B2 workspace (6,778 objects):

| Operation | Time |
| --- | --- |
| One `rclone cat` of a 4-byte file | **0.60 s** |
| Full recursive listing | 2.13 s |
| Same listing with `--fast-list` | 2.05 s (no benefit; do not pursue) |
| `tasks/`-scoped listing (10 files) | 0.59 s |

The ~0.6 s floor is process startup plus `b2_authorize_account`, not transfer.
A sync spawns roughly 8–12 rclone processes (identity probe, check-access
markers, bisync, the `tasks/` state probe, the batched CSV download/upload, and
four counter `copyto` calls), so several seconds of every sync is authentication
overhead that does no work.

Three earlier reductions have already landed: the task-state probe now lists
`tasks/` instead of the whole remote, the CSV phase batches its transfers into
one download and one upload, and identical lifecycle artifacts are no longer
rewritten (which was tripping the workspace watcher and causing a spurious push
on every TUI launch). What remains is the per-process authentication itself.

The candidate is a long-lived rclone: `rclone rcd` exposes a local HTTP control
API (`operations/copyfile`, `operations/list`, `sync/bisync`) against a single
authenticated process. Brain already owns a shared-server lifecycle with
election, generations, and orderly shutdown, so the daemon should very likely be
owned by that machinery rather than a second, parallel process manager.

## Measured after the first three reductions landed

A no-change `brain sync` on the author's `brain` workspace is **19.4 s**, of
which an equivalent `rclone bisync --dry-run` (both listings, no transfers) is
**17.0 s**. Brain's own remaining overhead is therefore only ~2.4 s, and this
task can recover at most that.

That measurement was then chased down and the cause found: rclone's default
march lists **per directory**, so on a bucket backend every one of the ~1,000
directories was its own API round trip. `--fast-list` replaces that with one
recursive listing per side and has landed. Measured on the same workspace:

| | Time |
| --- | --- |
| Dry-run bisync, per-directory march | 15.6 s |
| Dry-run bisync, `--fast-list` | **6.9 s** |
| Whole no-change `brain sync`, before | 19.4 s |
| Whole no-change `brain sync`, after | **7.2 s** |

Two levers were measured and rejected: `--checkers 32` made no difference
(7.7 s), and excluding the large media library was *slower* (9.5 s), so object
count is not the constraint — round trips were.

**What remains for this task is the ~2 s of per-process authentication**, now a
much larger share of a 7 s sync than it was of a 19 s one. Worth doing, but
measure again first: with `--fast-list` in place the bisync step is ~5 s of the
7 s, so a daemon that only removes brain's own process spawns still leaves most
of it.

## Acceptance criteria

- [ ] A decision is recorded in `docs/decisions.md` on whether the rclone daemon is owned by the existing shared server, by the sync lock holder, or spawned per sync run.
- [ ] Remote operations in one sync share a single authenticated rclone, measured as a reduction in process spawns and in wall-clock for a no-change sync.
- [ ] The `run_rclone` / `run_rclone_capture` seam keeps an injectable boundary so the existing local-transport and fake-remote tests stay hermetic.
- [ ] A daemon that dies mid-sync degrades to per-call invocations rather than failing the sync.
- [ ] Credentials never reach a command line or a written rclone config; they stay in the child environment as they do today.
- [ ] The `--max-delete` guard, `--check-access` marker, conflict naming, and CSV/counter out-of-band reconciliation all behave identically.
- [ ] `docs/integrations.md` and `docs/architecture.md` describe the daemon's lifecycle and its failure modes.

## Notes

Also worth folding in while here: the four counter `copyto` calls can join the
CSV phase's batched download/upload, since `tasks/.tasks_next_id` and
`tasks/.habits_next_id` live in the same remote directory that phase already
stages. That is a smaller, independent win that does not need the daemon.
