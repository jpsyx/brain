---
id: BR-30
title: Stop the one-second full-tree stat walk on macOS
status: todo
priority: medium
assignee: jpsyx
labels: [enhancement, performance, sync]
estimate: 3
project: PROJ-2
milestone: MS-9
cycle:
parent:
github:
blocked_by: [BR-21]
created: 2026-09-05
updated: 2026-09-05
---

# BR-30: Stop the one-second full-tree stat walk on macOS

## Description

On macOS the workspace watcher is `notify::PollWatcher` with a **one-second
poll interval**, recursive over the whole workspace root and unfiltered by the
sync exclude set. On a 145 GB tree that is a full recursive stat walk every
second, for the entire lifetime of the shell, which is usually all day. It is
the largest untouched suspect for both "sync feels slow" and a warm machine, and
no proposal in the original investigation touched it.

The poll watcher was chosen deliberately: the code comment records that FSEvents
can silently omit changes in otherwise valid user-owned trees on some macOS
versions, and the receiver needs a deterministic fallback without adding a
Watchman dependency. That reasoning stands, so this is not "switch back to
FSEvents and hope". Three shapes are available and should be measured rather
than argued:

1. **Raise the interval.** A 1-second poll on a tree Brain itself writes into is
   far tighter than the 3-second debounce downstream of it, so most of the work
   is discarded anyway. This is the cheapest change and may be sufficient.
2. **Filter the walk.** `is_watch_relevant` filters *events*, after the walk has
   already stat'd everything. Applying the exclude set to the walk itself is the
   real fix for cost; the media-heavy `resources/` tree dominates the object
   count.
3. **FSEvents primary with the poll watcher as a slow fallback.** Keeps the
   determinism the comment asks for while making the common case free. More code
   and a real behavior change, so it needs the measurement to justify it.

Measure first. Without a number this task is speculation, and the investigation
deliberately labelled it a suspect rather than a finding.

## Acceptance criteria

- [ ] The current cost is measured before any change: CPU time and wall time attributable to the watcher over a fixed window on the 145 GB workspace, recorded in the task notes.
- [ ] The chosen shape is justified by that measurement, and the rejected shapes are recorded with their numbers so the choice is auditable later.
- [ ] Change detection still works: a relevant edit under the root still triggers a sync within a stated bound, asserted by a test on the pure debounce and relevance logic rather than by a sleep.
- [ ] The deterministic-fallback property the existing comment relies on is preserved or explicitly traded away with reasoning; the receiver depends on not missing changes.
- [ ] If the poll interval becomes configurable, it satisfies the repo's CLI and command-palette parity rule; if it stays a constant, that is stated as a deliberate choice.
- [ ] The exclude set and the watch-relevance predicate do not drift apart. They already duplicate the same knowledge in two places (`src/sync/args.rs` `EXCLUDES` and `is_watch_relevant`); a guard test should tie them together if the walk starts consuming the exclude set.
- [ ] `docs/architecture.md` and `docs/decisions.md` C4 record the watcher's real cost profile and why the platform choice is what it is, replacing the current comment-only rationale.

## Notes

Last in the project by design. Also worth measuring in the same sitting, since
it is the same class of problem and needs a number before anyone optimizes it:
`src/sync/check/mod.rs` reuses the bisync argv and appends
`--compare size,checksum`, and rclone keeps no local hash cache, so `brain
check` plausibly reads all 145 GB per invocation. If that measures badly it
deserves its own task rather than being folded in here.

BR-6 (reuse one rclone process per sync) is the remaining speed item after this
one.

### Pointers (as of 2026-09-05)

- `src/sync/watch.rs` — the `#[cfg(target_os = "macos")]` `PollWatcher` selection, its `with_poll_interval(Duration::from_secs(1))`, the comment explaining the FSEvents choice, and `is_watch_relevant`. Everything in shapes 1 to 3 starts here.
- `src/sync/config.rs` — `debounce_ms` and `SyncConfig::debounce`, the downstream coalescing window the poll interval should be reasoned against. Note BR-26 also changes this default.
- `src/sync/args.rs` — the `EXCLUDES` array, the knowledge `is_watch_relevant` duplicates. Read its doc comment before adding a third copy.
- `src/sync/check/mod.rs` — the `--compare size,checksum` addition to the bisync argv, for the secondary measurement noted above.
- `docs/decisions.md`, search `## C4:` for the "watcher is an in-process shell thread, not a daemon" record; the platform-watcher choice belongs alongside it.
- [docs/investigations/2026-09-05-sync-self-conflict-loop.md](../../investigations/2026-09-05-sync-self-conflict-loop.md) — the Speed section, which flags this as the largest untouched suspect.

### Log

- 2026-09-05 created from the investigation.
