# Brain sync C3.4: `brain check` CSV diff Implementation Plan

**Goal:** Make `brain check` report pending row-level changes in
`tasks/tasks.csv` and `tasks/habits.csv`, using the same cached baselines and
remote `copyto` path as the C3 CSV sync lane, without mutating any files.

**Architecture:** Keep `src/sync/check.rs` as a read-only shell around pure
reporting helpers. Reuse `csv_merge::parse` for id-keyed CSV tables and
`csv_sync::{baseline_path, remote_csv_arg}` for baseline/remote naming. The only
IO added to `run` is baseline/local reads and best-effort remote reads.

---

## Scope

In scope: `brain check` report construction, pure CSV row diffing, read-only CSV
fetching, docs, status handoff. Out of scope: CSV merge behavior, baseline
mutation, conflict resolution, watcher triggers, or new dependencies.

## File Structure

| File | Responsibility |
| --- | --- |
| `src/sync/check.rs` | Pure CSV diff/report model plus the read-only check shell |
| `src/sync/csv_sync.rs` | Export the managed CSV list for reuse by check |
| `docs/features.md` | User-visible `brain check` behavior |
| `docs/integrations.md` | rclone/baseline read-only CSV check details |
| `docs/architecture.md` | Module responsibility update |
| `docs/data-model.md` | CSV pending-diff model |
| `docs/decisions.md` | Design rationale for row-delta check output |
| `docs/superpowers/brain-sync-status.md` | Mark C3.4 done after merge |

## TDD Steps

- [x] **RED:** Add a pure unit test proving CSV diff counts added, changed, and
  deleted rows between a baseline and one side.
- [x] **GREEN:** Implement the minimal `CsvSideDiff` / row-diff helper using
  `csv_merge::parse`.
- [x] **RED:** Add a pure unit test proving `CsvPending` captures local push
  rows and remote pull rows independently.
- [x] **GREEN:** Implement `CsvPending` construction from baseline, local, and
  optional remote text.
- [x] **RED:** Add a report-format test proving CSV row summaries affect the
  push/pull headings and `brain sync` suggestion.
- [x] **GREEN:** Extend `format_report` to accept CSV pending entries while
  preserving existing file-lane summaries.
- [x] **RED:** Add a shell-level unit test for the read-only collector with an
  injected remote fetcher.
- [x] **GREEN:** Implement the collector over `csv_sync::CSVS`,
  `baseline_path`, local reads, and injected/real remote fetch.
- [x] **Docs + validation:** Update docs, run `cargo test --release`, run
  `cargo clippy --release --all-targets`, commit, merge to `main`, delete the
  branch, and update the status handoff with the commit SHA.
