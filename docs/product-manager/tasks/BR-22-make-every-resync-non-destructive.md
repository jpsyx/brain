---
id: BR-22
title: Distinguish rclone's out-of-sync abort and make every resync non-destructive
status: todo
priority: urgent
assignee: jpsyx
labels: [bug, sync]
estimate: 5
project: PROJ-2
milestone: MS-6
cycle:
parent:
github:
blocked_by: [BR-21]
created: 2026-09-05
updated: 2026-09-05
---

# BR-22: Distinguish rclone's out-of-sync abort and make every resync non-destructive

## Description

This is the data-loss bug. It fired in production on 2026-09-05 at 10:47 with
nothing crashed, and it is reachable on an automatic trigger.

Two defects compose:

**The classifier collides on a substring.** `parse_outcome` in
`src/sync/run/mod.rs` matches `lc.contains("run --resync")` to detect a missing
prior listing. rclone's *listing-validation* failure reads `path1 and path2 are
out of sync, run --resync to recover`, which contains that literal. So a
validation failure is handled as an absence failure, and `should_auto_resync`
fires the recovery path. Measured across roughly 180 process logs:
`cannot find prior` appears **0 times**; `path1 and path2 are out of sync`
appears 4 times. Every `PriorListingMissing` classification Brain has ever made
came through this collision.

**The recovery destroys the loser.** Brain passes a bare `--resync`
(`src/sync/args.rs`). rclone documents that as `--resync-mode path1`, and
`--resync-mode` appears nowhere in `src/`. Reproduced with Brain's exact flag
set: `--conflict-loser pathname` is **not honored during a resync** and
`--conflict-resolve newer` is ignored, so the remote-newer file is overwritten
and no copy survives on either side.

```
before   path1/doc.txt = "LOCAL-older"
         path2/doc.txt = "REMOTE-NEWER-IMPORTANT"
$ rclone bisync ... --conflict-resolve newer --conflict-loser pathname --resync
after    both sides = "LOCAL-older"    (remote content gone, no marker anywhere)

with --resync-mode newer:  both sides = "REMOTE-NEWER-IMPORTANT"   (correct)
```

The journal note for the real run said only *"auto-resumed after interrupted
baseline."* It never said local overwrote remote. Silence on a lossy decision is
its own defect and is in scope here.

**The implementation trap.** `--resync-mode newer` triggers a resync **on its
own**, with no `--resync` flag present (verified: rclone logs `Resync is copying
files to...`). `bisync_args` appends nearly every flag unconditionally and gates
only `--resync` behind the `Direction::Resync` branch. Adding `--resync-mode`
the same way would turn *every* ordinary sync into a resync, destroying delta
detection and silently discarding the older side on every run. It must go
strictly inside that branch, with a test asserting its absence for Both, Pull,
and Push.

## Acceptance criteria

- [ ] A new `AbortKind` distinguishes rclone's listing-validation failure ("path1 and path2 are out of sync") from a genuinely missing prior listing, and `parse_outcome` no longer matches the former via a `run --resync` substring.
- [ ] `should_auto_resync` does not fire for the validation failure. That state is surfaced to the user with an accurate message, not auto-healed, matching the project's existing posture of surfacing rather than auto-healing anything that touches data loss.
- [ ] Pure unit tests cover both rclone messages verbatim and assert the two classifications differ; the existing `PriorListingMissing` tests still pass for the message they were written for.
- [ ] Every resync Brain runs passes `--resync-mode newer`.
- [ ] `--resync-mode` is present **only** for `Direction::Resync`. A test asserts its absence in the argv for Both, Pull, and Push, because passing it unconditionally makes every run a resync.
- [ ] The journal note for any resync states plainly that the older side of N differing files was discarded with no copy kept, so a lossy recovery is never reported as a bare "auto-resumed".
- [ ] `docs/decisions.md` records why the validation failure is surfaced rather than auto-resynced, as an amendment to the existing C2 "no silent auto-resync" record and the C3 narrowing of it.
- [ ] `docs/architecture.md` (the `sync/` section describes the auto-resume) and `docs/features.md` agree with the new behavior.

## Notes

Ship this before BR-29's cleanup. `brain sync repair` is the same bare
`--resync`, so cleaning up the existing conflict copies first would trigger
exactly this bug.

### Pointers (as of 2026-09-05)

- `src/sync/run/mod.rs` — `AbortKind`, `parse_outcome`, and its substring ladder. The whole fix for the classifier half is here; it is pure and already unit-tested, so this is a clean red/green.
- `src/sync/command/reporting.rs` — `should_auto_resync` and `should_auto_repair_check_access`, both pure predicates over `(Direction, Option<&AbortKind>)`. Add the new kind here.
- `src/sync/args.rs` — `bisync_args`. Note how `--resync` is the one flag gated behind `dir == Direction::Resync` near the end; `--resync-mode` must join it inside that same guard, never above it.
- `src/sync/command/mod.rs` — `sync_once`'s auto-resync block and the journal note assembly (the `resumed` / `auto_repaired` string joining). The honest-note criterion lands in the note builder.
- `src/sync/verify.rs` — `classify` turns the run into `Clean` / `NeedsAttention` / `Aborted`. A resync that discarded a side must not report `Clean`.
- `docs/decisions.md`, search `## C2/§19` for "Why there is no silent auto-resync (partially superseded)" and the C3 narrowing. This task narrows it further; write the amendment there rather than a new disconnected entry.
- [docs/investigations/2026-09-05-sync-self-conflict-loop.md](../../investigations/2026-09-05-sync-self-conflict-loop.md) — steps 2 and 3 of the causal chain carry the log excerpts and the reproduction commands.

### Log

- 2026-09-05 created from the investigation. Highest-priority item in PROJ-2.
