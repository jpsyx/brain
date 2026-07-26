# Brain sync C3 — id-keyed CSV semantic merge — design

- **Date:** 2026-07-25
- **Status:** Design — ready for plan + build. Phase C3 of Sub-project C.
- **Scope:** replace keep-both conflict copies for `tasks.csv` / `habits.csv` with a
  **row-id-keyed 3-way merge**, so add / complete / delete / field-edit from two
  machines converge cleanly and these files never spawn `(conflict …)` copies.
  Builds on C2 (bisync transport, journal, `~/.cache/brain/sync/`).

---

## 1. Why

The two task CSVs are edited constantly and are *structured, id-keyed* data — the
worst fit for whole-file keep-both. Two machines that each add a task, or one
completes while the other adds, should **merge**, not conflict. C2 puts them in the
normal file lane (keep-both fallback); C3 gives them a semantic merge.

## 2. Shape

- **Exclude** `tasks/tasks.csv` and `tasks/habits.csv` from the Lane-A bisync (add
  to the `EXCLUDES` filter in `args.rs`). bisync never touches them.
- brain syncs each CSV itself with a **3-way merge**: `base` (last-synced snapshot,
  cached) + `ours` (local) + `theirs` (remote), keyed by `task_id`, producing the
  merged CSV written to **both** sides. This runs as a step of `brain sync` (and
  `check` reports pending CSV changes too).

## 3. The merge (the crown jewel — pure, exhaustively tested)

`merge(base, ours, theirs) -> (merged_rows, Report)`, all keyed by `task_id`:

- **Row in `ours`/`theirs` but not `base`** → *added* → keep it. (Added on both with
  the same id but different content → field-merge per below.)
- **Row in `base`, absent from one side** → *deleted* on that side → **delete** from
  the merge (honors deletions; a delete beats an unrelated edit on the other side —
  or, safer default, *delete wins only if the other side didn't change it*; if the
  other side edited it, keep the edited row and journal a "deleted-vs-edited" note).
- **Row present in both, unchanged vs `base` on one side** → take the other side's
  version (that side made the only change).
- **Row changed on both sides** → **field-level merge**, per column:
  - column unchanged vs `base` on one side → take the other side's value;
  - both changed the *same* column to *different* values → **last-writer-wins by the
    row's `last_touched` timestamp** (tasks have it — §4); **completion always wins**
    (any side that set `status=done` wins the `status`/`completed_date` fields);
  - both changed a column to the *same* value → that value.
- Output rows sorted deterministically (by `task_id`) so both machines write byte-
  identical results → **convergence**.

`Report` carries counts (added/deleted/merged) and any soft notes (deleted-vs-edited,
un-resolvable same-field with no timestamp) for the journal + `brain sync status`.

## 4. Convergence timestamp — `last_touched`

`tasks.csv` already had `last_touched`; C3.3 extended the same column to
`habits.csv`. The merge uses it as the per-row modified time for same-field
last-writer-wins on both CSVs. A row whose `last_touched` is empty/unparseable
falls back to the deterministic value tiebreak (the lexicographically-greater
value wins) so legacy or damaged rows still converge and journal a soft note.

**Writers must bump `last_touched`.** Whatever mutates `tasks.csv` or
`habits.csv` (the `/todo` skill scripts, the `/habits` server path through
`mark_done.py`, and brain's own writes) sets `last_touched` to now on every row
change, or same-field LWW degrades to the fallback above.

## 5. Transport + baseline

- **Baseline snapshots** live at `~/.cache/brain/sync/baselines/{tasks,habits}.csv`
  (machine-local cache, not synced). After a successful CSV sync, the merged content
  is snapshotted as the new base. **First run (no baseline)** → `base` = empty ⇒ every
  row is "added" ⇒ the merge is the union of local + remote (safe, never loses).
- **Fetch remote** via `rclone copyto b2:<bucket>/<path>/tasks/<name>.csv <temp>`
  (using the env-var remote from C2). Missing remote file → `theirs` = empty.
- **Write back**: merged → local file; `rclone copyto <local> b2:…/<name>.csv` →
  remote. Update the baseline.
- **Ordering**: run the CSV merge as a step of `brain sync` after Lane-A bisync (so
  everything else is reconciled first). `brain sync --push/--pull` bias only affects
  Lane A; the CSV merge is always a true 3-way merge (biasing a semantic merge makes
  no sense — it always converges).

## 6. Module layout

- `src/sync/csv_merge.rs` (new) — the **pure** merge: parse rows (reuse the `csv`
  crate), the id-keyed 3-way algorithm, deterministic serialize. No IO.
- `src/sync/csv_sync.rs` (new) — the thin IO: baseline read/write, fetch remote via
  rclone, write local + push, call the pure merge, journal the report.
- Wire into `src/sync/command.rs::sync_once` (a CSV-merge step) and exclude the CSVs
  in `args.rs`.
- `brain check`: extend to also report pending CSV changes (a dry-run merge diff), so
  the summary covers tasks/habits too.

## 7. Testing (pure-first; this is the highest-value test surface in the project)

Exhaustive `csv_merge` unit tests: add-on-both-sides, complete-vs-edit, delete-vs-
unchanged, delete-vs-edit (the soft-note path), field-level union (different fields),
same-field LWW by `last_touched`, completion-always-wins, both-set-same-value,
empty-baseline union, **idempotent re-merge** (merging an already-merged pair is a
no-op), and **convergence** (merge(A,B) and merge(B,A) yield byte-identical output).
The IO shell (`csv_sync`) is thin; exercised by a gated integration test that merges
two local CSVs through the real path (no B2).

## 8. Docs

`docs/features.md` (tasks/habits merge cleanly, no conflict copies), `docs/data-model.md`
(the `last_touched`-driven merge + baseline schema), `docs/integrations.md` (CSVs
excluded from bisync + the copyto merge path), `docs/decisions.md` (id-keyed merge over
keep-both; reuse `last_touched`; convergence rules), the `AGENTS.md` docs-contract row.

## 9. Acceptance

1. `tasks.csv`/`habits.csv` are excluded from bisync and never produce `(conflict …)`
   copies.
2. Add/complete/delete/different-field edits from two machines all converge; a
   same-field divergence resolves by `last_touched` LWW, with a deterministic
   journalled fallback for legacy/no-timestamp rows.
3. `merge` is deterministic + convergent + idempotent (property-tested).
4. Baselines cached in `~/.cache/brain/sync/baselines/`; first-run union is safe.
5. `brain check` reports pending CSV changes; the journal records CSV merge outcomes.
6. `cargo test --release` green; `cargo clippy --release --all-targets` clean.

## 10. Decomposition (for the plan)

- **C3.1 — the pure merge** (`csv_merge.rs`): parse → id-keyed 3-way → deterministic
  serialize, with the full test matrix. No IO. (The bulk of the value.)
- **C3.2 — baseline + transport** (`csv_sync.rs`): baseline cache, rclone fetch/push,
  write-local; wire into `sync_once`; exclude CSVs from bisync.
- **C3.3 — writers set `last_touched`**: audit `mark_done.py` + task writers; ensure
  every mutation stamps it (fallback stays safe if not).
- **C3.4 — `brain check` CSV diff + docs.**
