---
id: BR-25
title: Retire --conflict-resolve path2 from every automatic sync trigger
status: todo
priority: high
assignee: jpsyx
labels: [bug, sync]
estimate: 3
project: PROJ-2
milestone: MS-7
cycle:
parent:
github:
blocked_by: [BR-21]
created: 2026-09-05
updated: 2026-09-05
---

# BR-25: Retire --conflict-resolve path2 from every automatic sync trigger

## Description

`args::bisync_args` maps `Direction::Pull` to `--conflict-resolve path2`, which
means the remote wins any conflict unconditionally. That is correct for a human
typing `brain sync --pull` (an explicit "give me the remote's version"). It is
wrong for an automatic trigger, where it means the stale remote silently beats
the edit the user just made locally.

Measured over 2,939 journal runs: `push` produced 0 conflict copies across
1,370 runs; `pull` produced 1,766 across 1,525. Every conflict Brain has ever
created came from a bidirectional run, and the overwhelming majority from the
`path2` bias.

The nuance that makes this task necessary rather than obvious: a genuinely
local-only edit is *not* a conflict and is pushed up even in Pull direction
(verified). The demotion only happens once a path is already in the
baseline-eviction state described in the investigation, at which point it reads
as "new on both sides" and `path2` decides in the stale remote's favor. So this
is not the root cause, but it is the step that turns a recoverable state into a
conflict copy, and it fires every five minutes.

**Five call sites use `Direction::Pull`, not one.** Fixing only the timer leaves
the failure on the startup path, which runs immediately after a machine was
edited offline, the worst possible moment:

| Site | Trigger |
| --- | --- |
| `src/sync/periodic.rs` | the five-minute reconcile |
| `src/tui/runtime/builder.rs` | TUI startup pull |
| `src/tui/app_sync.rs` | receiver freshness pull |
| `src/command/tasks.rs` | non-TUI tasks startup pull |
| `src/workspace/initialize.rs` | emptied-root recovery |

The "Pull avoids uploading mid-edit state" objection does not apply:
`Direction::Pull` already runs a full bisync (only `Push` uses `push_args`), so
it already uploads local changes and already propagates local deletes. Pull and
Both differ **only** in the conflict bias. Everything else is
direction-insensitive: the CSV and counter lanes branch on `!= Push`, and the
journal's downstream-freshness queries already include `both`.

Two shapes are available. Either convert every automatic trigger to
`Direction::Both`, or introduce a distinct direction so `path2` becomes
reachable only from a typed `--pull`. The second is more code but makes the
invariant structural, which suits a bug that recurred because one call site was
easy to miss.

## Acceptance criteria

- [ ] No automatic sync trigger runs with `--conflict-resolve path2`. All five call sites are covered, enumerated in the change and asserted by tests.
- [ ] An explicitly typed `brain sync --pull` still resolves in the remote's favor; that is the flag's documented meaning and it does not change.
- [ ] A test makes the invariant hard to regress: either the argv builder cannot produce `path2` for an automatic direction, or a guard test enumerates every `Direction::Pull` construction site and asserts each is user-initiated.
- [ ] The receiver freshness gate still works. `message_pull_due` reads the journal for the last downstream run, and the journal's direction filter must include whatever direction replaces Pull.
- [ ] The emptied-root recovery path still recovers: it needs remote content to arrive, which a bidirectional run does, but confirm it does not now push a local emptiness upward before the remote lands.
- [ ] `docs/decisions.md` C4 is amended to record why the periodic reconcile is no longer a Pull, and `docs/features.md` plus `docs/architecture.md` agree on what each automatic trigger does.

## Notes

Watch the interaction with BR-26. If a trigger's direction changes and
coalescing still drops runs, the freshness gate can go quiet in a new way;
land them in the same milestone and test them together.

### Pointers (as of 2026-09-05)

- `src/sync/args.rs` — `bisync_args`, the `Direction` enum, and the `resolve` match that maps Pull to `path2`. Both candidate shapes start here.
- `src/sync/periodic.rs` — `PULL_INTERVAL` and `spawn_periodic_puller`; the five-minute timer, one line.
- `src/tui/runtime/builder.rs`, `src/tui/app_sync.rs`, `src/command/tasks.rs`, `src/workspace/initialize.rs` — the other four sites. Grep `Direction::Pull` to confirm the list has not drifted before starting.
- `src/sync/trigger.rs` — `detached_sync_args` translates a `Direction` into the child's argv (`--pull` / `--push`). A new direction needs a flag mapping and a test here; the existing tests assert exact argv.
- `src/sync/freshness.rs` and `src/sync/journal.rs` — `message_pull_due` and the journal queries that decide what counts as a downstream run. Check the direction filter includes the replacement.
- `src/sync/command/reporting.rs` — `direction_label`, `direction_from_flags`, `sync_progress`; user-facing strings and the flag parser both need the new direction if one is added.
- `docs/decisions.md`, search `## C4:` for the live-shell auto-sync record that explains why the periodic run is a Pull today.

### Log

- 2026-09-05 created from the investigation.
