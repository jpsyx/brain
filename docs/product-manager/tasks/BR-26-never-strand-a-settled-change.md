---
id: BR-26
title: Never strand a settled change behind a coalesced trigger
status: todo
priority: high
assignee: jpsyx
labels: [bug, sync]
estimate: 5
project: PROJ-2
milestone: MS-7
cycle:
parent:
github:
blocked_by: [BR-21]
created: 2026-09-05
updated: 2026-09-05
---

# BR-26: Never strand a settled change behind a coalesced trigger

## Description

A settled local change can go unsynced indefinitely. This was filed in the
investigation as a speed item and is really a durability one.

`--if-idle` **drops** the run rather than queueing it: when the workspace sync
lock is held, `run_with_workspace_lock` returns `WorkspaceLockOutcome::Coalesced`
and the child exits silently. Meanwhile `run_watcher_loop` disarms the debouncer
the moment it fires, and `spawn_detached_sync` returns only a pid, so the parent
never learns the child coalesced. The result: a change that settles while a sync
is in flight is stranded until the next filesystem event or the next periodic
run.

`docs/decisions.md` C4 asserts the opposite ("The watcher's Debouncer re-arms
after a skip, so pending changes aren't stranded"). The doc is wrong and must be
corrected as part of this change.

A second half, observed and unquantified: a pull writes files **under the
watched root**, so each writing pull provokes follow-on pushes. Measured on
2026-09-05, 15 detached children in five minutes, and per-minute, a writing pull
provoked 4 to 5 pushes while a quiet pull provoked 1. The `watch.rs` doc comment
only argues that *push* mode is self-safe, which is true and beside the point.
Suppressing that loop needs a mechanism, and `notify` carries no writer
identity, so the in-process watcher can only gate on observing the sync's own
state (the lock, or `sync/current`). Pick one and say which.

The 3-second `default_debounce_ms` is also far too tight for a tree Brain itself
writes into. Raising it is part of this task, and per the repo's CLI and
command-palette parity rule, any new live toggle needs both surfaces.

## Acceptance criteria

- [ ] A coalesced trigger is never lost: either the intent is recorded and retried when the lock frees, or the debouncer stays armed so the next poll refires. A pure test asserts "a trigger that coalesces leads to a later sync without requiring a new filesystem event."
- [ ] At most one pending sync intent per workspace exists at a time; a burst of settles does not spawn a child per settle.
- [ ] The pull-then-push feedback loop is suppressed by a named mechanism, and that mechanism is documented. Writes performed by Brain's own sync do not provoke a follow-on push.
- [ ] The debounce default is raised to a justified value, with the reasoning recorded rather than a bare number.
- [ ] Any new live toggle satisfies the repo's CLI and command-palette parity rule: a startup surface (a `brain config` / `brain env` variable, or a flag if genuinely per-invocation) and a palette row that writes the same `App` field through the same pure decision.
- [ ] `docs/decisions.md` C4's stranding claim is corrected in the same change, since it currently states the bug is impossible.
- [ ] `docs/features.md` and `docs/architecture.md` describe the trigger lifecycle accurately, including what happens when a trigger coalesces.

## Notes

Sequence with BR-25. If both the direction and the coalescing behavior change,
the receiver freshness gate's notion of "a downstream sync happened recently"
shifts on two axes at once; test them together.

### Pointers (as of 2026-09-05)

- `src/command/sync.rs` — `run_with_workspace_lock` and `WorkspaceLockOutcome`. The `Coalesced` arm is where the run is dropped today; this is the smallest place to see the bug.
- `src/sync/watch.rs` — the pure `Debouncer` (`on_event`, `time_until_fire`, `poll`), `run_watcher_loop`, `is_watch_relevant`, and `spawn_watcher`. The re-arm decision is pure and belongs in `Debouncer`, which already has sleep-free tests to extend.
- `src/sync/trigger.rs` — `spawn_detached_sync` and `DetachedSyncRunner`. The runner is already an injectable trait, so a "one pending intent per workspace" gate is testable without spawning processes.
- `src/sync/lock.rs` — `try_acquire` is non-blocking by design. Whatever mechanism suppresses the pull-then-push loop probably observes this lock or `sync/current`; read `src/sync/current.rs` for what the running sync already publishes.
- `src/sync/config.rs` — `default_debounce_ms` and `SyncConfig::debounce`. Note `debounce_ms` is already a portable config field, which decides which half of the parity rule it satisfies.
- `src/tui/palette/` and `src/menu/model.rs` — where a palette row and its dimmed shortcut hint must be registered if a live toggle is added.
- `docs/decisions.md`, search `## C4:` for the stranding claim to correct.
- [docs/investigations/2026-09-05-sync-self-conflict-loop.md](../../investigations/2026-09-05-sync-self-conflict-loop.md) — the trigger-storm table in the Speed section.

### Log

- 2026-09-05 created from the investigation, reclassified from speed to correctness after review.
