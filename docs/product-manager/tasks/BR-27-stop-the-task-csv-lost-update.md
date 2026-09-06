---
id: BR-27
title: Stop the task-CSV lost update from silently deleting rows
status: todo
priority: urgent
assignee: jpsyx
labels: [bug, sync]
estimate: 13
project: PROJ-2
milestone: MS-8
cycle:
parent:
github:
blocked_by: [BR-21]
created: 2026-09-05
updated: 2026-09-05
---

# BR-27: Stop the task-CSV lost update from silently deleting rows

## Description

Brain can silently delete a task. This is the only genuinely structural item in
PROJ-2 and it is unrelated to the conflict noise that prompted the
investigation.

Two facts compose badly:

**The workspace sync lock is machine-local.** `WorkspacePaths::sync_lock()`
resolves under `~/.cache/brain/workspaces/<uuid>/`, so it serializes syncs on
one laptop and nothing across machines. With a five-minute timer firing from
every open shell on every machine, concurrent runs are the steady state rather
than a rare race.

**The CSV lane is an uncoordinated read-modify-write.**
`csv_sync/transport.rs` does `batch_download`, then the merge, then
`batch_upload`, against one shared remote object. B2 exposes no
compare-and-swap, so a second machine's upload between another's read and write
is simply lost.

The lost update then becomes a deletion. `csv_merge/merge.rs` reads
"present in base, unchanged locally, absent remotely" as a remote delete:

```rust
(Some(base), Some(side), None) | (Some(base), None, Some(side)) => {
    if side == base {
        report.deleted += 1;   // row dropped
    } else {
        rows.insert(id.to_owned(), side);
    }
}
```

So: machine A publishes a merged CSV containing row X. Machine B, reading before
A's write landed, republishes without X. On A's next sync, X is in A's baseline,
unchanged locally, and absent remotely, so A drops it. The task is gone from
both machines and nothing reports it.

Contrast `src/sync/counters.rs`, which is a monotonic max-join and structurally
immune to the same interleaving. The difference is that one lane infers meaning
from absence and the other does not. That is the real lesson and it belongs in
the decision record.

Two candidate shapes, both in-house, no new service:

1. **A remote lease.** Reuse the append-only, distinct-name,
   lowest-UUID-election protocol that `sync/identity/claim.rs` already
   implements for workspace claims. That protocol exists precisely because B2
   offers no portable compare-and-swap, so the hard part is already solved and
   tested in this codebase.
2. **Single-writer-per-object state.** Each machine owns
   `tasks/.state/<machine-uuid>.csv` and never writes another machine's file.
   The merge becomes a join over all of them, and a deletion becomes an explicit
   tombstone rather than an inference. This removes the race by construction
   rather than by mutual exclusion, and it composes with BR-28's machine id.

Option 2 is the stronger answer and the larger change. Option 1 is smaller and
reuses proven code. Decide during implementation with the RED test already
written, since the test is the same either way.

## Acceptance criteria

- [ ] A RED test lands first: a two-machine interleaved-upload harness that reproduces the lost update and asserts a row present locally and absent remotely is never dropped without an explicit tombstone. Written and observed failing before any fix.
- [ ] After the fix, that harness passes, and the existing `csv_merge` convergence and idempotency tests still pass unchanged. Those two properties are the safety guarantee for this lane and must not regress.
- [ ] A deletion is stated, never inferred. Either the lane can no longer observe a partial remote state, or absence alone stops meaning "deleted" and a tombstone is required.
- [ ] Concurrent syncs from two machines cannot interleave a read and a write on the same remote object, or the design makes interleaving harmless. Whichever is chosen is stated explicitly in `docs/decisions.md`.
- [ ] The chosen mechanism degrades safely: a lease that cannot be acquired defers the CSV lane and says so, rather than proceeding unprotected. A failure to reconcile never publishes a partial merge, preserving the existing whole-operation preflight guarantee.
- [ ] No new cloud service, and no dependency beyond what the crate already has.
- [ ] The habits lane and the id counters are covered by the same reasoning, with the counters explicitly noted as already immune and why.
- [ ] `docs/decisions.md` records the "never infer intent from absence" rule as a durable design principle, contrasting the CSV lane with the counter max-join, plus `docs/architecture.md`, `docs/data-model.md`, and `docs/integrations.md` updated per the docs contract.

## Notes

Priority is urgent despite the large estimate: unlike the conflict noise, this
one loses data with no artifact left behind, so it is invisible until someone
notices a missing task. It has no known reproduction in the wild yet, only in
review, which is exactly why the harness matters.

### Pointers (as of 2026-09-05)

- `src/sync/csv_merge/merge.rs` — the match arms above; the delete inference is the defect's expression. Read the convergence and idempotency tests in `csv_merge/mod.rs` first, since they define what any change must preserve.
- `src/sync/csv_sync/transport.rs` — `batch_download` and `batch_upload`, the read-modify-write. The rclone shell is injectable, so a fake transport can drive the interleaving harness without the network.
- `src/sync/csv_sync/operation.rs` — the whole-operation preflight and publication order. The "never publish a partial merge" guarantee lives here and must survive.
- `src/sync/identity/claim.rs` — the append-only claim protocol with distinct names and lowest-UUID election. This is the in-house answer to "B2 has no compare-and-swap" and is the template for the lease option.
- `src/sync/counters.rs` — the monotonic max-join. The contrast case; read it to see what immunity looks like.
- `src/workspace/paths.rs` — `sync_lock()` and the `cache_dir` derivation that makes the lock machine-local. Confirm before designing.
- `docs/decisions.md`, search `## C3 —` for the CSV merge record, including its explicit note that "The CSV lane has no per-machine author field", which BR-28 changes.
- [docs/investigations/2026-09-05-sync-self-conflict-loop.md](../../investigations/2026-09-05-sync-self-conflict-loop.md) — "The one genuinely structural gap".

### Log

- 2026-09-05 created from the investigation's architecture review.
