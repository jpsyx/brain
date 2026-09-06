---
id: BR-21
title: Decide the sync investigation's open questions
status: todo
priority: urgent
assignee: jpsyx
labels: [chore, sync]
estimate: 2
project: PROJ-2
milestone: MS-5
cycle:
parent:
github:
blocked_by: []
created: 2026-09-05
updated: 2026-09-05
---

# BR-21: Decide the sync investigation's open questions

## Description

The 2026-09-05 sync investigation produced a fix plan but left decisions that
are the user's to make, not the implementer's. Every task in PROJ-2 is blocked
on this one, because two of the decisions change what gets built and one
determines whether an existing task survives at all.

This is a decision task: its deliverable is recorded answers, not code. Write
each answer into the place that will still be read a year from now (the target
task file, or `docs/decisions.md` when it is a genuine design choice), then
close this.

### D1. BR-11 or BR-20? They ship opposite defaults.

[BR-11](BR-11-interactive-sync-conflict-resolution.md) makes a conflict prompt
the user per file and hand the remainder to an LLM.
[BR-20](BR-20-never-leave-a-conflict-unresolved.md) makes Brain resolve every
conflict automatically and never ask. Both cannot be the default.

Recommendation: BR-20's default wins, and BR-11 is rescoped to an explicit,
opt-in review pass over the ledger (an LLM proposing a semantic merge for rows
already resolved by `lww-register`, which is safe precisely because the losing
bytes are still recoverable). Cancel BR-11 outright if that rescope has no
value once the ledger exists.

### D2. Is the resolution ledger portable or machine-local?

BR-20 currently assumes portable: `<brain-root>/.conflict-resolutions.sqlite`
as specified, excluded from bisync and reconciled by a union merge on
`resolution_id`, the same shape `csv_sync` already uses for the two CSVs. The
alternative is machine-local under `<workspace-cache>/sync/`, which drops the
union lane entirely and is meaningfully less code.

The tradeoff: portable means a forced resolution on the laptop is visible and
rollback-able from the desktop; machine-local means the record only exists where
the decision was made.

### D3. Confirm the landing order.

MS-6 before MS-7 before MS-8 before MS-9, and specifically the cleanup of the
existing conflict copies **after** the non-destructive resync. Confirm, or say
what you want reordered and why. This is a gate because `brain sync repair` is
a bare `--resync` today, so cleaning up first is the data-loss event.

### D4. What shape does the per-machine identity take?

BR-28 needs a decision on the clone problem. A machine cloned by Migration
Assistant, a Time Machine restore, or `rsync ~/.config` inherits the same id,
which is strictly worse than hostname drift because nothing can detect it.
Options: bind the mint to a hardware fingerprint (`IOPlatformUUID`) and re-mint
when it changes, or accept duplicate ids and document the hazard.

Recommendation: bind to the fingerprint. The failure it prevents is silent.

### D5. Which of the deferred findings actually get built?

The investigation lists findings that became BR-22 through BR-30. Confirm all
of them are wanted, or cut. The two most likely cut candidates are BR-30 (the
watcher stat walk, if you would rather just raise the poll interval by hand and
move on) and BR-29 (the cleanup, if you would rather resolve the 44 copies
manually in Finder now that they are inventoried).

## Acceptance criteria

- [ ] D1 is decided and recorded: BR-11 is either rescoped in its own file (with its acceptance criteria rewritten) or set `status: cancelled` with a `superseded-by: BR-20` note and archived.
- [ ] D2 is decided and recorded in BR-20's description, replacing its stated assumption; if machine-local wins, BR-20's union-merge acceptance criterion is struck in the same edit.
- [ ] D3 is confirmed or amended, and PROJ-2's milestone order matches the answer.
- [ ] D4 is decided and recorded in BR-28, including what happens on a detected fingerprint change.
- [ ] D5 is decided; any cut task is set `status: cancelled` with a one-line reason and archived, so the board reflects reality.
- [ ] Any answer that is a genuine design choice rather than a scheduling call also lands in `docs/decisions.md`, per the repo's docs contract.

## Notes

### Pointers (as of 2026-09-05)

- [docs/investigations/2026-09-05-sync-self-conflict-loop.md](../../investigations/2026-09-05-sync-self-conflict-loop.md) — the source. Its "Candidate fixes, adjudicated" table and "Findings not yet filed as tasks" section are what D3 and D5 are deciding over.
- [PROJ-2](../projects/PROJ-2-make-sync-conflict-free.md) — the milestone order D3 confirms, and the scope statement that records what was deliberately rejected.
- `docs/decisions.md` — where a design answer belongs. Sections C2/§19 and C3 to C5 are the existing sync records; a new entry should be written as an explicit supersession rather than an addition when it reverses one.
- `docs/product-manager/config.md` — no counter changes needed for a decision task, but cancelling a task per D1 or D5 means moving its file to `archive/`.

### Log

- 2026-09-05 created as the gate for PROJ-2.
