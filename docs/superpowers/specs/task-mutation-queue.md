# Optimistic task mutations

## Problem and scope

The TUI currently completes tasks synchronously, including waiting for the
task-store lock and updating the agenda. A sync pull owns that same lock, so
confirming completion can freeze input and rendering. Deletion already uses a
background worker, but its single-operation coordinator keeps the modal open
and cannot safely track multiple accepted operations. Its preview also says
"Working on task deletion" before the user has confirmed anything.

Move native TUI completion and deletion through one process-local FIFO. Keep
the CLI synchronous and preserve its existing transaction and consent checks.
This change covers task and habit completion, task deletion, and deletion
preview/recovery. It does not move every unrelated TUI operation into a worker.

## User experience

1. Completion keeps its confirmation. Deletion keeps Cancel selected by
   default, the existing policy choices, and both `d` and Down then Enter.
2. Once confirmed, the modal closes immediately. The task is marked done or
   removed from the displayed list immediately, according to the current view.
   The UI continues accepting input while persistence waits for sync.
3. A persistent pending-work indicator distinguishes queued changes from saved
   changes. Success is reported after the native operation commits.
4. A failed operation restores only its own optimistic change and reports the
   affected task and reason. Other queued changes remain intact.
5. Deletion preview is read-only and cancelable. While it waits, its copy says
   that Brain is checking deletion details and that nothing has been deleted.
   `d` cannot confirm a preview that has not finished or has blockers.
6. Normal quit requests while accepted changes remain explain that saving is
   pending and keep the TUI responsive. Once saving settles, quitting works
   normally. Read-only previews alone do not prevent quitting.

## Ownership and data flow

```text
Confirmed command
  +--> Pending effect --> Authoritative rows + effects --> Task list
  |
  +--> FIFO --> Wait for lock --> Native operation
                                   |
                                   v
                            Commit outcome + rows
                                   |
                                   v
                            Settle one effect --> Task list

Sync / manual refresh --> Replace authoritative rows
                          Keep pending effects --> Task list
```

The event loop owns UI state and submits owned requests. A native worker owns
only a pinned command context and request data. It does not borrow the App,
terminal, database service aggregate, or agent controller. At most one native
job executes at a time. Requests retain FIFO order. All requests, including read-only previews, retain their order; accepted
mutations and recovery requests are never discarded.

Each job has an independent monotonically increasing identity. Accepted-job
metadata outlives its modal. Modal generations fence stale preview rendering,
but never discard a confirmed mutation's result. Opening another modal cannot
lose completion, rollback, or a recoverable deletion operation.

## Authoritative state and optimistic projection

Keep the latest authoritative task and habit rows separate from pending effects.
Each effect belongs to one job and one immutable task identity. Deletion hides
the target. Completion projects status, completed date, and last-touched date;
native execution remains responsible for recurrence, MIT transfer, dependent
edits, agenda changes, and other derived writes.

Every replacement of authoritative rows reapplies pending effects. This applies
to sync completion, manual refresh, agent refresh, view changes, and worker
results. Rollback removes a job's effect and rematerializes the latest base;
it never restores an old whole-list snapshot. Preserve the user's selection by
identity when possible, otherwise use the nearest surviving row. Do not revive
a pending deletion in target pickers or let repeated completion create duplicate
jobs for one occurrence.

Capture workspace, actor, CSV kind, task UUID, and confirmation date with a job.
Capture completion identity when the confirmation opens, so a refresh before
Enter cannot redirect the action to another row with the same display ID.
Legacy rows use the existing workspace-scoped deterministic UUID convention.
Resolve completion against its UUID under task-store ownership. An already-done
occurrence is idempotent and must not spawn another habit occurrence.

## Locking and deletion consent

Keep the shared task-store lock and sync's consistency boundary. Waiting happens
only in workers. Provide a queued acquisition mode that retries typed SQLite
busy/locked acquisition failures, including after the ordinary timeout; other
I/O, recovery, validation, and authorization failures settle as errors. Do not
retry an entire mutation after it may have committed or invoked an external
action. Existing CLI lock acquisition behavior remains bounded.

Deletion preview still obtains a consistent owned snapshot. Confirmed deletion
uses its captured plan and fingerprint. At execution, keep the existing full
revalidation, dependent policy, project edits, linked-issue semantics, and
durable receipts. Changed consent rolls back the optimistic effect and requires
a fresh preview and confirmation. Never silently regenerate a plan and apply it.
Serializing previews with mutations normally lets each preview observe earlier
queued changes; external writers may still invalidate any preview.

## Settlement and failures

| Outcome | UI result |
| --- | --- |
| Not committed | Remove only that job's effect, reconcile current rows, report failure |
| Committed | Adopt current canonical rows and remove the effect |
| Committed with agenda or refresh warning | Keep the committed change visible; report the warning |
| Stale deletion plan | Restore visibility, require explicit preview and confirmation |
| Recoverable deletion failure | Retain the operation ID and recovery controls independently of newer jobs |
| Worker disconnect | Reconcile commit evidence without replay; retain an explicitly uncertain effect until a successful authoritative refresh when evidence cannot be read |

Refresh errors must not resurrect a known committed deletion or undo a known
completion. Preserve a committed projection until an authoritative refresh can
represent it, and show the refresh problem. Worker-side refresh work must not
introduce a lock wait or agenda subprocess on the event loop. A later unrelated
success must not silently erase an unresolved failure or recovery obligation.

## Lifetime and limits

The queue belongs to one running TUI. Ordinary quit is guarded while accepted
work remains, so it cannot silently discard queued changes. Finished worker
handles are reaped without blocking rendering. Forced termination or a crash
can lose unstarted in-memory intents; existing durable removal operations and
task-store transaction recovery retain their current guarantees. A durable
outbox and cross-process intent replay are outside this change and are not
implied by the pending indicator.

## Verification

Use pure tests for projection, identity, ordering, settlement, and quit policy.
Use controlled worker responses for exact App lifecycle assertions, and real
temporary workspace locks for the native boundary. Tests must prove immediate
modal dismissal and projected rows while the lock is held, input remains usable,
FIFO persistence after release, sync refresh cannot undo optimism, isolated
rollback, UUID safety, idempotent habit completion, stale deletion consent,
committed warnings, worker failure, and pending-work quit behavior. Maintain the
existing shortcut/palette and architecture guards. Run release tests and Clippy
before publication.

### Snapshot ordering and retained recovery identity

Each queued request captures an authoritative-refresh revision. If an independent
refresh has replaced rows before its result is consumed, its snapshot is rejected
and one coalesced asynchronous snapshot is queued through the same FIFO. Job
settlement still completes, committed effects remain projected, and fresh
reconciliation exposes derived rows without replacing newer edits with old rows.
Existing operation requests retain their captured recovery identity on read or
retry errors. Only a new Apply known to fail before operation creation releases
its preallocated operation identity.
