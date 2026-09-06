---
id: BR-23
title: Never mutate the synced root while a sync holds the workspace lock
status: todo
priority: urgent
assignee: jpsyx
labels: [bug, sync]
estimate: 8
project: PROJ-2
milestone: MS-6
cycle:
parent:
github:
blocked_by: [BR-21]
created: 2026-09-05
updated: 2026-09-05
---

# BR-23: Never mutate the synced root while a sync holds the workspace lock

## Description

Brain writes into the synced root while its own bisync is running. rclone
builds both listings, Brain then rewrites files underneath them, and rclone's
listing validation fails with a hard abort. Reproduced from production logs:

```
/tmp/2026-09-05T10:47:27...-94371.log   (direction=pull)
10:47:28  INFO : Building Path1 and Path2 listings
10:47:30  INFO : Path1 checking for diffs
10:47:45.76      <- brain re-renders .agents/skills/** under the root
10:47:47  ERROR: Modtime not equal in listing. Path1: 14:47:45.762...   (x33)
10:47:47  ERROR: Bisync critical error: path1 and path2 are out of sync
```

BR-22 makes the *consequence* of that abort non-destructive. This task removes
the *cause*, and it is the only change that makes "a machine never conflicts
with its own change" mechanically true rather than probabilistically true.
BR-24 reduces how often the race is armed but does not close it: any in-root
write racing a sync produces the same abort.

The workspace sync lock already exists (`src/sync/lock.rs`) and is already the
serializer for every sync entry point. What is missing is that Brain's other
in-root writers do not consult it. The barrier should be a real seam rather than
a convention, so a future writer cannot forget: route in-root artifact writes
through a guard that either waits for the lock or defers the write, and make the
guard the only sanctioned way to write inside a workspace root.

Design questions to settle while implementing, not before:

- **Wait or defer?** Waiting risks blocking TUI startup behind a 7-second sync.
  Deferring risks a stale artifact for the length of a sync. A reasonable split
  is to defer (skip, then reconcile on the next invocation) since these writers
  are already reconciliation passes that run on every invocation, so a skipped
  write self-heals.
- **Scope.** The barrier covers writes *inside a workspace root*. Writes to the
  UUID cache, `~/.config/brain/`, and `/tmp` are unaffected and must stay
  unaffected, or startup deadlocks against itself.
- **The sync's own writes are exempt.** `csv_sync` publishes into the root while
  holding the lock, by design. The guard must permit the lock holder.

## Acceptance criteria

- [ ] A pure decision function answers "may this path be written now?" from the workspace root, the target path, and whether this process holds that workspace's sync lock; it is unit-tested without touching the filesystem.
- [ ] Every Brain writer that targets a path inside a workspace root goes through the guard. The skills pipeline, the lifecycle/hook artifact writers, the check-access marker, and the reindex lookup CSVs are all covered; an audit of in-root writes is part of the change.
- [ ] The sync process itself is not blocked by its own lock: the CSV and counter publication lanes still write while holding it.
- [ ] Writes outside a workspace root (the UUID cache, `~/.config/brain/`, temporary directories) are unaffected, with a test asserting the guard does not apply to them.
- [ ] A deferred write is not silently lost: it either reconciles on the next invocation by construction, or it is retried, and the choice is recorded in `docs/decisions.md`.
- [ ] A deferred write logs why at the normal verbosity, so "my hooks did not update" is diagnosable without `--verbose`.
- [ ] An integration-level test proves the invariant: with a simulated in-flight sync holding the lock, an artifact-installing code path does not modify the root.
- [ ] `docs/architecture.md` and `docs/integrations.md` describe the barrier as a contract for future writers, and `docs/decisions.md` records why a convention was insufficient.

## Notes

The deeper framing worth capturing in the decision record: the root is shared
mutable state between Brain's own subsystems and an external transport that
takes a point-in-time snapshot of it. Every such writer needs to respect the
snapshot window, and the only durable way to enforce that is a seam, not a
comment.

### Pointers (as of 2026-09-05)

- `src/sync/lock.rs` — the existing UUID-scoped advisory lock, with `try_acquire`, PID-liveness reaping, and a heartbeat. It is non-blocking today; a barrier that waits needs a bounded blocking acquire, which is a new entry point rather than a change to the existing one.
- `src/workspace/paths.rs` — `sync_lock()` and the `cache_dir` derivation. Read this to see exactly which paths are inside the root versus the cache; the guard's scope predicate keys off that distinction.
- `src/command/server/receiver/hooks/artifact.rs` — `write_workspace_artifact`, `resolve_confined_write_destination`, and `write_static_file`. This is already the closest thing to a sanctioned in-root writer (it confines paths to the workspace and skips identical content), so it is the natural place to host the guard.
- `src/skills/install.rs` — `write_built_to` and `write_fresh_built_to`; the largest in-root writer and the one that produced the observed race. See BR-24, which is the churn half of the same file.
- `src/sync/check_access.rs` — `ensure_local_marker` writes `RCLONE_TEST` into the root on every non-Push sync. Note it runs *from inside* the sync, so it is a lock-holder case, not a barrier case.
- `src/reindex/walk.rs` — rebuilds `projects/projects-lookup.csv` and `resources/zotero-lookup.csv` in the root. A genuine third-party writer that must respect the barrier.
- `src/startup_migration/lifecycle.rs` — `install_workspace_hooks` and `install_active_session_shims`, which fan out across every registered workspace root on every invocation. The multi-workspace fan-out means the guard must be per-workspace, not global.
- `src/sync/csv_sync/operation.rs` — the sync's own in-root publication, the case that must stay permitted.
- [docs/investigations/2026-09-05-sync-self-conflict-loop.md](../../investigations/2026-09-05-sync-self-conflict-loop.md) — step 1 of the causal chain, with the full log excerpt.

### Log

- 2026-09-05 created from the investigation. Pairs with BR-22; that one makes the consequence safe, this one removes the cause.
