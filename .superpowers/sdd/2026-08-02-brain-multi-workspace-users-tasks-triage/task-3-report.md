# Task 3 report: workspace-aware task assignment and mutators

## Status

Complete on `feat/workspace-users`. The implementation, documentation, version
bump, standalone Python coverage, and Rust integration coverage are ready for
parent-agent review. It has not been pushed or merged.

## Delivered

- Added canonical `Task::assigned_to` normalization for tasks and habits.
  Readers accept legacy `assignee`; when both headings exist, `assigned_to`
  wins even when it is intentionally blank.
- Added pure assignment decisions:
  - creation defaults to the immutable effective actor for both one-person and
    shared workspaces;
  - unrelated edits preserve the current assignment;
  - explicit reassignment parses a portable `UserId` and requires membership;
  - one-person workspaces hide assignment detail, creation/reassignment
    controls, and filtering, while shared workspaces expose all four surfaces.
- Made native complete, revive, and skip runners accept explicit
  `WorkspaceContext` and `ActorContext`. They use the selected context root and
  do not re-resolve global state. Reindex already had this exact explicit
  context/actor contract, so it required no production change.
- Made the shared Rust CSV reader/writer normalize assignment headings by
  column name before every mutation. Legacy-only files retain the value at the
  same column position under `assigned_to`; files containing both headings
  discard only the legacy heading/value and retain the canonical value.
- Centralized bundled Python context in `_csvlib.py`: `brain_root()`,
  `actor_id()`, `tasks_csv()`, `habits_csv()`, UUID creation, assignment
  membership validation, and canonical CSV migration.
- Removed every task-script fallback to `Path.home() / "brain"`. Missing or
  non-absolute `BRAIN_ROOT` and missing `BRAIN_ACTOR_ID` now fail directly with
  guidance to launch through Brain.
- Replaced task creation's `--assignee` option with `--assigned-to`. Omission
  uses `BRAIN_ACTOR_ID`; explicit assignment reads the selected workspace's
  `.config/users.json` and validates portable membership.
- Added `reassign_task.py` for explicit validated reassignment. It preserves
  unrelated fields, stamps `last_touched`, and benefits from the same legacy
  header migration.
- Routed all affected task scripts, including task/habit IDs, cleanup,
  triage-state files, backlog tools, Linear-link updates, agenda refresh, and
  project lookups, through the selected workspace helpers.
- Added a fully isolated Python subprocess suite that places a decoy Brain
  under the temporary home directory and proves it remains untouched.
- Updated the todo and triage skills plus architecture, features, data model,
  integrations, decisions, and testing documentation.
- Bumped Brain from `0.19.2` to `0.20.0` in `Cargo.toml` and `Cargo.lock`.

## RED and GREEN transcript

### Rust reader compatibility

The first focused reader run failed to compile because `Task` did not expose
the requested field:

```text
$ cargo test --release task_reader_ -- --nocapture
error[E0609]: no field `assigned_to` on type `Task`
  --> src/tasks/task/load.rs:242:25
error[E0609]: no field `assigned_to` on type `Task`
  --> src/tasks/task/load.rs:252:25
```

Adding only the normalized field and legacy/canonical reader behavior turned
both tests green. Compilation then exposed direct `Task` fixtures in
`src/tui/app_actions/triage.rs` and `src/tui/tests/links.rs`; those fixtures
received the required empty assignment without changing behavior.

Final review added a stricter regression test for an intentionally blank
canonical value when both columns exist. The old fallback behavior failed:

```text
task_reader_preserves_blank_assigned_to_when_both_columns_exist ... FAILED
assertion `left == right` failed
  left: "legacy"
 right: ""
```

Serde normally treats an absent optional column and a present blank cell the
same. A narrow field deserializer now preserves that distinction. The final
reader run passed all three cases:

```text
running 3 tests
task_reader_accepts_legacy_assignee_as_assigned_to ... ok
task_reader_prefers_assigned_to_when_both_columns_exist ... ok
task_reader_preserves_blank_assigned_to_when_both_columns_exist ... ok
```

### Pure assignment decisions

The initial assignment test run failed with unresolved imports for
`assignment_for_create`, `assignment_after_edit`, and `assignment_ui_mode`.
The new single-responsibility `task/assignment.rs` module made creation,
preservation, membership validation, and one-versus-many visibility tests
green. The final task-module run passed 28 tests before the final reader edge
test and 29 task-module tests within the final complete suite.

### Rust writer migration

The first unrelated-completion test retained the legacy header:

```text
left:  "task_id,task_name,status,assignee,completed_date,last_touched\n..."
right: "task_id,task_name,status,assigned_to,completed_date,last_touched\n..."
```

A second RED case containing both columns retained both `assignee` and
`assigned_to` instead of only the canonical column. Header normalization in
the shared CSV load seam turned both cases green while preserving the canonical
value and all unrelated fields. The final complete-module suite passed 11
tests.

### Explicit Rust command contexts

Compile-contract tests initially failed with `E0308` for complete, revive, and
skip because each public runner still accepted a `Path` where the test required
`&WorkspaceContext`. After threading the selected workspace through the two
command dispatchers, all three contracts passed. A scoped source search found
no `paths::brain_root` calls in complete, revive, skip, reindex, or their
dispatchers.

### Isolated Python context and assignment

The first six-test Python run produced five failures and one error:

- selected-root creation wrote no selected-root file;
- seven bundled scripts still contained a home-Brain fallback;
- `_csvlib.new_uuid` did not exist;
- `add_task.py` rejected `--assigned-to`;
- missing `BRAIN_ROOT` exited successfully through a fallback;
- legacy writer output still lacked `assigned_to`.

Centralized context and CSV migration turned all six green. The explicit
reassignment test then failed RED because `reassign_task.py` did not exist
(Python exit 2). Adding the focused mutator turned the final standalone suite
green at seven tests.

The existing Rust Python-subprocess suite initially exposed two integration
contract issues: macOS Python 3.9 eagerly evaluated modern union annotations,
and the old fixtures did not provide the new immutable Brain environment.
Future annotations in the shared import path plus a common isolated
workspace/actor command fixture restored all five existing integration tests.

### Verification fixes

The first strict Clippy run reported `assigning_clones` in the header migration
and `no_effect_underscore_binding` in three compile-contract tests.
`clone_into` and no-op typed helper functions corrected those findings; the
strict rerun passed.

The first isolated full-suite command hid rustup's installed default toolchain
along with test home state. The corrected isolated command retained explicit
`CARGO_HOME` and `RUSTUP_HOME` while isolating Brain-owned home, XDG, cache, and
temporary data. It passed 1,162 tests before the final canonical-blank reader
case was added. The post-review complete release suite then passed 1,163 tests.

## Final verification

- `PATH="/opt/homebrew/bin:$PATH" cargo test --release`: 1,163 tests passed
  (1,009 library tests plus all integration and documentation tests).
- `/usr/bin/python3 -m unittest discover -s skills/todo/scripts/tests -v` on
  Python 3.9.6: 7 passed.
- `cargo test --release --test todo_script_mutators`: 5 passed.
- `cargo test --release bundled_skills_carry_no_personal_data`: passed.
- `cargo clippy --release --all-targets -- -D warnings`: passed.
- `rustfmt --edition 2024 --check` over every changed Rust file: passed.
- `git diff --check`: passed.
- Scoped searches found no task-script `Path.home() / "brain"` fallback, no
  personal absolute path, and no in-scope `paths::brain_root` call.
- The standalone suite leaves Python bytecode disabled; an earlier generated
  `__pycache__` was inspected and removed before handoff.

## Known repository-level verification issue

Repository-wide `cargo fmt --all -- --check` under the installed formatter
reports extensive pre-existing drift in untouched files. The first reported
file is `src/command/server/receiver/hooks/tests.rs`, followed by many other
unrelated paths. No Task 3 change relies on that baseline: every Rust file
changed here passes the explicit edition-2024 formatter check. An initial
scoped check with edition 2021 also could not parse the repository's existing
edition-2024 let-chain in `src/command/tasks.rs`; the correct edition-2024 check
is green.

## Files changed

- Version: `Cargo.toml`, `Cargo.lock`
- Rust model/decisions: `src/tasks/task/{mod,load,assignment}.rs`
- Rust mutation/context: `src/tasks/{complete,revive,skip}.rs`,
  `src/command/tasks.rs`, `src/command/server/habits.rs`
- Rust fixtures/integration: `src/tui/app_actions/triage.rs`,
  `src/tui/tests/links.rs`, `tests/todo_script_mutators.rs`
- Python core/new coverage: `skills/todo/scripts/{_csvlib,add_task,reassign_task}.py`,
  `skills/todo/scripts/tests/test_workspace_context.py`
- Python workspace routing: `apply_sync_rules.py`,
  `cleanup_done_habits.py`, `dedupe_backlog.py`,
  `find_chronic_ignored.py`, `find_stale_waiting.py`, `list_backlog.py`,
  `list_linked_tasks.py`, `monthly_triage_state.py`,
  `next_habit_occurrence.py`, `next_id.py`, `purge_old_backlog.py`,
  `set_linear_issue.py`, `track_late_work.py`, and
  `update_agenda_on_mutation.py`
- Skills: `skills/todo/SKILL.md`, `skills/todo/references/{commands,schema}.md`,
  `skills/triage/SKILL.md`
- Product docs: `docs/{architecture,data-model,decisions,features,integrations,testing}.md`

## Self-review

Assignment uses portable user IDs only. It does not introduce owner, creator,
device, audit, authentication, or authorization semantics. Creation uses the
already-resolved immutable actor; explicit changes validate the selected
workspace's portable membership; unrelated mutations preserve assignment.
Legacy compatibility is read-only and asymmetric, with every future writer
converging on one canonical heading. All bundled root resolution is explicit,
and missing context fails instead of crossing workspace boundaries.

The UI behavior delivered in this task is the pure visibility/default decision
specified by the brief and its tests; wiring additional concrete TUI controls
is outside this task's listed production surfaces. Reindex already accepted
and propagated both contexts, so changing it would have been unnecessary
churn. No other concerns remain.

## Commit

Commit subject: `feat(tasks): add workspace-aware assignments`. The exact hash
is reported in the parent handoff because a commit cannot contain its own hash.
No push or merge is performed.
