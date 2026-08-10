---
id: BR-7
title: Skip the task/habit merge phase when neither side changed
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

# BR-7: Skip the task/habit merge phase when neither side changed

## Description

Every sync runs the task/habit reconciliation unconditionally: it stages
`SCHEMA.json` and both CSVs from the remote, merges by `task_uuid`, publishes the
result, then reconciles both ID counters. On a workspace where nothing changed,
that whole phase is pure overhead — and it is the phase a user notices, because
it runs after the file sync has already reported nothing to do.

Batching (BR-6's landed half) reduced it to one download plus one upload plus
four counter calls. Skipping it entirely when neither side changed removes the
rest.

The check has to cover **both** sides, and "bisync moved nothing" is not
sufficient evidence: the CSVs are deliberately excluded from bisync, so another
machine can have changed them while bisync reported zero transfers.

The cheap way to know is already in hand. The task-state probe now lists
`tasks/` (BR-6), and that listing can carry size and modtime per file at no
extra cost. So:

- **Local unchanged** — the working CSV bytes equal the cached merge baseline
  under `<workspace-cache>/sync/baselines/`.
- **Remote unchanged** — the `tasks/` listing's size+modtime for each CSV equals
  a fingerprint recorded at the end of the last successful merge.

When both hold, skip the staging, the merge, the publication, and the counter
reconciliation, and say so in the sync output (BR-8's "found:" lines) so a
skipped phase is visibly a decision rather than a silent omission.

## Acceptance criteria

- [ ] A pure decision function takes (local-matches-baseline, remote-fingerprint-matches-recorded) and returns whether the merge phase runs, with tests for all four combinations.
- [ ] The remote fingerprint is persisted next to the CSV baseline and updated only after a successful publication, so an interrupted sync re-runs the phase rather than skipping it.
- [ ] A remote CSV changed by another machine still triggers the merge even when bisync transferred nothing.
- [ ] A locally edited CSV still triggers the merge even when the remote is untouched.
- [ ] A missing or unreadable fingerprint means "run the phase" (fail toward work, never toward skipping).
- [ ] The sync output names the decision and why, and the journal note distinguishes "skipped, nothing changed" from "merged, no rows differed".
- [ ] `docs/features.md` and `docs/data-model.md` describe the fingerprint and the skip rule.
