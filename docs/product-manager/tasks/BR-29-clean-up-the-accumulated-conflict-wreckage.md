---
id: BR-29
title: Clean up the accumulated conflict copies and remote orphans, with content verification
status: todo
priority: medium
assignee: jpsyx
labels: [chore, sync]
estimate: 5
project: PROJ-2
milestone: MS-9
cycle:
parent:
github:
blocked_by: [BR-21, BR-22]
created: 2026-09-05
updated: 2026-09-05
---

# BR-29: Clean up the accumulated conflict copies and remote orphans, with content verification

## Description

Three weeks of the self-conflict loop left wreckage on both sides, and the two
sides do not agree on what it is:

| Location | Item | Count |
| --- | --- | --- |
| `~/brain` | friendly `(conflict …)` copies | 44 (36 byte-identical, 8 differing) |
| `~/brain` | raw `__brainconflict__` markers | 5 |
| remote bucket | orphan `__brainconflict__` markers | 17 |
| remote bucket | friendly `(conflict …)` copies | 0 |

Friendly copies are a bisync exclude, so they stay local-only; raw markers
accumulate on the remote where no Brain command surfaces them. `docs/decisions.md`
C2 admits this leak ("one orphan object into the bucket per conflict, forever,
visible to no brain command").

**Three of the 17 remote markers are real user content**, stranded with no local
counterpart:

```
areas/avandar/legal-finance/policies/avandar-privacy-policy-v3-2026-06-24.docx.__brainconflict__1
projects/personal__build-diff-comment-style-guide/{.capture-state.json,log.jsonl,report.html}.__brainconflict__{1,2}
resources/email-digests/2026-08-25-07-38/2026-08-25-07-38-newsletters.md.__brainconflict__2
```

**This is not a bulk delete.** Two guards are mandatory:

1. **Blocked on BR-22.** `brain sync repair` is a bare `--resync` today, so the
   closing baseline rebuild would itself be the data-loss event. BR-22 must land
   first. This is recorded in `blocked_by`, not just prose.
2. **Content verification per file.** 8 of the 44 local copies hold content that
   differs from the original, and `brain sync resolve` deletes without a content
   check. Those 8 plus the real-content remote markers need a decision each, not
   a sweep.

Prefer building the verification as a reusable command over doing it by hand
once, since BR-20's migration criterion needs the same capability and a future
episode will need it again.

## Acceptance criteria

- [ ] A command reports every conflict copy and marker on both sides with a content comparison against its canonical original: identical, differing, or no-original. `brain sync conflicts --json` is the natural home; extend rather than add a parallel surface.
- [ ] The remote half is included. Today no Brain command surfaces a remote orphan marker, which is why 17 accumulated unnoticed.
- [ ] Byte-identical copies can be resolved in bulk, safely, on both sides.
- [ ] A differing copy is never deleted without an explicit per-file decision, and the losing content is preserved somewhere durable before deletion. If BR-20's ledger exists by then, that is where it goes; if not, this task states where.
- [ ] After the run, `~/brain` contains zero `(conflict …)` copies and zero `__brainconflict__` markers, and the remote contains zero markers, verified by a listing rather than asserted.
- [ ] The closing baseline rebuild runs only after BR-22, and the task fails closed with a clear message if the non-destructive resync is not present.
- [ ] `.claude/settings.json.bak` and any other stray artifact found during the sweep is inventoried and either kept deliberately or removed, rather than left unexplained.
- [ ] `docs/features.md` documents the verification surface, and `docs/decisions.md` C2's admitted remote-orphan leak is updated to say how it is now closed.

## Notes

Ordering within MS-9: run this before BR-30, since the watcher change alters
timing and a clean tree makes the speed measurement interpretable.

BR-21's D5 may cut this task in favor of resolving the 44 copies by hand now
that they are inventoried. If so, cancel it but keep the remote half, because 17
orphan markers on the remote are not hand-resolvable without a listing command.

### Pointers (as of 2026-09-05)

- `src/sync/conflicts/mod.rs` — `list_conflicts`, `group_conflicts`, `parse_conflict_name`, `marker_original`, and `remote_losers_for_original`. Both naming forms are already parseable, including the remote's raw markers; the content comparison is the missing piece.
- `src/sync/command/resolve.rs` and `resolve_remote.rs` — today's deleter, local and remote halves. `resolve_remote` already lists the original's own remote directory and deletes per-loser with `deletefile` rather than `delete`; reuse it and add the content gate above it.
- `src/sync/command/reporting.rs` — `conflicts_json` and `print_conflicts`, the surface to extend, plus `CopyMeta` and `conflict_display_paths` which already carry per-copy metadata.
- `docs/decisions.md`, search `## C5 —` for the structured-conflict-list record and the C2 passage admitting the remote-orphan leak. Both need updating.
- [BR-20](BR-20-never-leave-a-conflict-unresolved.md) — its migration acceptance criterion covers the same 44 copies. Decide which task owns the migration so it is not built twice.
- [docs/investigations/2026-09-05-sync-self-conflict-loop.md](../../investigations/2026-09-05-sync-self-conflict-loop.md) — "Current mess to clean up" has the full inventory and the stranded-content paths.

### Log

- 2026-09-05 created from the investigation.
