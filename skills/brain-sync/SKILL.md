---
name: brain-sync
description: Synchronize a Brain workspace across machines, inspect sync status, or resolve prose keep-both conflicts. Local lookup rebuilding belongs to organize.
---

# brain-sync

Own cloud synchronization and conflict resolution through `brain sync`.
For local lookup rebuilding use [`/organize reindex`](../organize/SKILL.md#reindex).

## Cloud sync

`/brain-sync`, `/brain-sync cloud-sync`, "sync", "do a sync", "sync my brain",
"pull latest brain changes", and "sync across machines" mean cloud sync.
Do not ask which kind when the user simply asks to sync.

1. Run `brain sync`. Honor an explicit directional request with
   `brain sync --push` or `brain sync --pull`.
2. If sync is not configured, explain that `brain sync setup` is the next step
   and stop. Otherwise report the actual command outcome, including failures
   or partially completed lanes. Never turn a failed transfer into a success.
3. Run `brain sync status`; report the last run and open-conflicts count.
   If conflicts remain, identify [Resolve conflicts](#resolve-conflicts).
   Say "in sync" only when the run and verification support it, not merely
   because the conflict count is zero. State any status-check failure separately.

## Resolve conflicts

`/brain-sync resolve-conflicts` handles prose keep-both `(conflict …)` copies.
Task and habit CSVs use id-keyed semantic merge during sync; residual CSV soft
conflicts live in the sync journal and are outside this prose-copy workflow.

1. List groups with `brain sync conflicts --json`. Each group supplies
   `original`, `original_exists`, and `copies` with `path`, `host`, `date`,
   `modified` (which can be null), and `bytes`. An empty array means no prose
   copies need resolving.
2. Read the canonical file if present and every competing copy. Use available
   recency evidence; union additive changes, preserve useful content, and prefer
   the newest `modified` on an actual incompatible passage. If recency is
   absent/equal or intent is consequentially unclear, resolve that uncertainty
   before deleting copies. If the original is missing, promote the best copy
   to the canonical path and fold in the rest.
3. Write and verify each merged canonical file before any loser cleanup.
   Process all groups first. If reading or writing any group fails, retain
   its copies and report incomplete merging; do not claim it resolved.
4. Once all groups are safely written, clear their copies in one batch:

   ```sh
   brain sync resolve <original1> <original2> ...
   ```

   Quote paths containing spaces. `resolve` refuses a missing canonical file,
   clears local and remote losers, and does not sync by itself. Do not manually
   remove remote objects. Report per-original outcomes and remote failures:
   `could not check the remote` means local cleanup may be committed while a
   remote loser survives; retry that original once connectivity returns.
   `no local copies, N remote objects` is a normal remote-orphan cleanup result.
5. Propagate with one final `brain sync`, then check `brain sync status`.
   Report merged files, remaining conflicts, and any cleanup/transfer failure.
   Do not conceal a partial result behind a blanket completion statement.
