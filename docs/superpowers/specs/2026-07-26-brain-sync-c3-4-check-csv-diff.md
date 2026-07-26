# Brain sync C3.4: `brain check` CSV diff design

- **Date:** 2026-07-26
- **Status:** Design, ready for plan + build. Final C3 follow-up.
- **Scope:** extend `brain check` so its read-only pending-change report includes
  `tasks/tasks.csv` and `tasks/habits.csv`, which are excluded from the rclone
  bisync lane and merged out-of-band by C3.

---

## 1. Why

C3 moved `tasks.csv` and `habits.csv` out of rclone bisync so they can use the
id-keyed 3-way merge. That makes `brain sync` safer, but it leaves `brain check`
with a blind spot: the existing dry-run bisync report can only see the normal
file lane. A user can have pending task or habit row edits and still see an
"In sync" report.

## 2. Behavior

`brain check` keeps its existing file-lane dry-run report and adds a CSV lane:

- For each managed CSV, compare the cached baseline with the local CSV to report
  rows that would be pushed.
- Fetch the remote CSV with the same rclone `copyto` transport used by
  `csv_sync.rs`, then compare the cached baseline with the remote CSV to report
  rows that would be pulled.
- Count row-level additions, changed rows, and deletions. The check is a diff,
  not a merge preview: it reports pending row movement without adjudicating
  field-level merge outcomes or writing anything.
- Missing local, remote, or baseline text is parsed as empty CSV text, matching
  the first-sync union behavior in `csv_sync.rs`.
- A remote CSV fetch failure should not hide local pending rows. The report skips
  that CSV's pull-side row diff and shows a themed warning that the remote CSV
  could not be checked.

The existing baseline-less bisync message still wins for a completely new or
interrupted rclone baseline. C3.4 is about filling the CSV gap once the normal
sync baseline exists.

## 3. Pure Model

Add a small pure diff model beside `check.rs`'s report builder:

- `CsvSideDiff`: row counts for `added`, `changed`, and `deleted`.
- `CsvPending`: the CSV name plus push and pull `CsvSideDiff` values.
- `diff_csv_rows(base, side)`: parse both texts with `csv_merge::parse`, key by
  `task_id`, and compare whole rows.
- `csv_pending_from_texts(name, base, local, remote)`: produce the push/pull row
  deltas for one CSV, with `remote = None` meaning remote was not checked.

The IO shell only reads baseline/local text and fetches remote text. It never
writes local files, remotes, or baselines.

## 4. Reporting

Keep the current report shape:

- "Changes to push" includes normal file summaries and CSV row summaries.
- "Changes to pull" includes normal file summaries and CSV row summaries.
- A CSV summary is compact and stable, for example
  `tasks.csv: +2 ~1 -0 rows`.
- The suggested command still says `brain sync`, with push/pull/all wording based
  on both file and CSV pending counts.

## 5. Docs

Update the same C3 docs surfaces as the merge itself:

- `docs/features.md` for user-visible `brain check` behavior.
- `docs/integrations.md` for the rclone `copyto` read-only CSV check.
- `docs/architecture.md` for `check.rs` module responsibility.
- `docs/data-model.md` for the baseline-vs-side row diff model.
- `docs/decisions.md` for why check reports row deltas rather than simulating a
  full merge.

## 6. Acceptance

1. `brain check` reports local task/habit row edits as pending push changes.
2. `brain check` reports remote task/habit row edits as pending pull changes
   when the remote CSV can be fetched.
3. Existing file-lane dry-run behavior and baseline guidance stay intact.
4. The CSV check is read-only: no local, remote, or baseline writes.
5. `cargo test --release` green; `cargo clippy --release --all-targets` clean.
