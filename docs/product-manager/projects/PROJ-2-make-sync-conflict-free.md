---
id: PROJ-2
name: Make sync conflict-free
status: planned
health: on-track
lead: jpsyx
members: []
initiative:
target_date:
github:
created: 2026-09-05
updated: 2026-09-05
---

# PROJ-2: Make sync conflict-free

## Goal

A machine must never see a conflict with its own change, and no automatic sync
path may silently discard an edit. Speed matters, but only after those two hold.

Origin: [docs/investigations/2026-09-05-sync-self-conflict-loop.md](../../investigations/2026-09-05-sync-self-conflict-loop.md).
Read that first; it carries the evidence, the reproductions, and the reasoning
for every task here, including the eleven candidate fixes that were adjudicated
and the five that were rejected.

## The finding in one paragraph

The rclone bisync engine is not the defect. Brain trips it by writing into the
synced root during its own run, mis-diagnoses the resulting listing-validation
abort as a missing baseline, and then "recovers" with a bare `--resync`
(`--resync-mode path1`) in which `--conflict-loser` is not honored, so the
remote-newer side is destroyed with no copy anywhere. Separately, rclone's own
conflict rename evicts the canonical path from both saved baselines, and the
five-minute automatic reconcile runs `--conflict-resolve path2`, so the stale
remote wins and the local edit is demoted to a conflict copy. Measured: `push`
produced 0 conflicts across 1,370 runs; `pull` produced 1,766 across 1,525.

## Scope

In scope:

- Close the destructive auto-resync path: distinguish rclone's out-of-sync
  abort from a missing baseline, make every resync non-destructive, and stop
  Brain from mutating the synced root while a sync holds the workspace lock.
- Stop manufacturing self-conflicts: make in-root artifact writes idempotent,
  retire `--conflict-resolve path2` from every automatic trigger, and stop
  stranding a settled change behind a coalesced trigger.
- Make cross-machine concurrency safe: the workspace sync lock is machine-local
  today, and the task-CSV lane is an uncoordinated read-modify-write against a
  store with no compare-and-swap, so a lost update is read as a remote delete
  and drops a task row.
- Give Brain a durable per-machine identity, so provenance is answerable at all.
- Clean up the accumulated wreckage (44 local conflict copies, 5 local markers,
  17 remote orphan markers) with content verification, never a bulk delete.
- Remove the largest speed cost found: a one-second recursive stat walk of a
  145 GB tree.

Out of scope:

- Replacing rclone bisync with a home-grown sync engine. Three adversarial
  reviews reproduced the engine behaving as documented; the defects are in
  Brain's configuration of it and in Brain racing it.
- Content-addressed blob storage, per-file revision trees, or CRDTs over opaque
  files. See the prior-art table in the investigation for why each is rejected.
- Any new cloud service or third-party sync engine. Backblaze B2 stays the only
  remote; everything here is in-house.
- The `rclone rcd` daemon. Already tracked as BR-6 and deliberately ordered
  after correctness.

## Product contracts

- A machine never conflicts with its own change.
- No automatic sync path discards an edit without recording it.
- A completed sync leaves no unresolved conflict on either side.
- A deletion is stated, never inferred from absence.
- Brain does not write into the synced root while a sync for that workspace is
  in flight.
- A settled local change is never stranded unsynced.
- Speed work follows correctness work, never precedes it.

## Ordering hazard (load-bearing)

The milestones are a required sequence, not a preference. `brain sync repair`
is a bare `--resync` today, so cleaning up the existing conflict copies before
MS-6 lands would itself be the data-loss event. Likewise MS-7's churn fixes
reduce the frequency of the MS-6 race but do not close it, so they must not be
mistaken for the fix.

## Milestones

- **MS-5: Decisions** (target: before any implementation): resolve the open
  questions the investigation raised, including the BR-11 versus BR-20 conflict
  of defaults and where the resolution ledger lives. Nothing below starts until
  these are answered.
- **MS-6: Close the destructive path**: the abort misclassification, the
  non-destructive resync, and the write barrier. This is the data-loss fix and
  it lands first.
- **MS-7: Stop manufacturing self-conflicts**: idempotent in-root writes,
  retire `path2` from automatic triggers, and never strand a settled change.
- **MS-8: Cross-machine safety**: the task-CSV lost update, and a durable
  per-machine identity.
- **MS-9: Cleanup and speed**: content-verified cleanup of the accumulated
  conflict copies and remote orphans, then the watcher stat-walk cost.

## Related work outside this project's task list

- **BR-20** (never leave a conflict unresolved; LWW plus a rollback ledger).
  Depends on MS-6 and MS-7 landing first, or its ledger records thousands of
  Brain-versus-Brain rows instead of real user divergence.
- **BR-11** (interactive conflict resolution with an LLM handoff). Ships the
  opposite default to BR-20. BR-21 decides which survives.
- **BR-6** (reuse one rclone process per sync). The remaining speed item after
  MS-9, deliberately last.

## Status updates

- **2026-09-05, planned, on-track.** Project opened from the investigation.
  Ten tasks across five milestones, 54 points. Nothing started; MS-5 is the
  gate. Two of the investigation's own proposals were rejected for causing data
  loss and are recorded as rejected in the investigation rather than as tasks,
  so they do not get re-proposed later.
