# Task sync rules

Canonical reference for the automations that derive / validate /
mutate `~/brain/tasks/{tasks,habits}.csv`. Mirrors the four Notion
formulas (`Is Done`, `Is MIT`, `Past due`, `Next Due`) the old
Tasks Tracker used, plus the local-only rules this system adds.

Run via:

- `/todo reindex` — applies all rules to tasks.csv + habits.csv.
- `/second-brain reindex` — same, plus projects + zotero. See
  `../second-brain/SKILL.md`.
- `python3 ~/.agents/skills/todo/scripts/apply_sync_rules.py [--fix]`
  — dry-run by default; pass `--fix` to write corrections.

## Derived (computed on read; not stored)

1. **`is_done`** — `status == 'done'`.
2. **`is_mit`** — `'mit'` in the pipe-split `task_type`.
3. **`past_due`** — `due_date < today` AND `status != 'done'`. Habits
   compute the same way; a missed habit is past-due until completed
   or until its next instance spawns.
4. **`is_visible_today`** — (`start_date == ''` OR `start_date <=
   today`) AND `status NOT IN ('done', 'backlog')`. Used to hide
   deferred tasks (future `start_date`), completed tasks, and parked
   `backlog` tasks from "Today" / "What should I work on" / "Agenda"
   views.
4a. **`is_stale`** — `(today - last_touched) >= 21` AND `status NOT IN
    ('done', 'backlog')`. Tasks-only even though habits also carry
    `last_touched`; habit recurrence controls their freshness.
    Backlog tasks can't be stale — they're parked on purpose.
4b. **`is_stuck_in_progress`** — `status == 'in_progress'` AND
    `(today - last_touched) >= 14`.
4c. **`is_captured_forgotten`** — `(today - created_date) >= 60` AND
    `status == 'not_started'` AND empty `notes`, `estimated_duration`,
    `project`.
4d. **`is_chronic_ignore`** — `(is_stale OR is_stuck_in_progress OR
    is_captured_forgotten)` AND `NOT past_due` AND (`due_date == ''`
    OR `due_date > today + 14`). The set surfaced by
    [`scripts/find_chronic_ignored.py`](../scripts/find_chronic_ignored.py)
    and swept in `/triage` Step 7. Past-due rows are excluded
    because they're handled in Steps 1–4; rows due in the next
    14 days are excluded because they're handled in Step 6.

## Mutations (apply_sync_rules.py with `--fix`)

5. **`completed_date` auto-set** — when `status == 'done'` and
   `completed_date` is empty, set to today.
6. **`defer_count` default** — empty → `0`.
7. **Habit cleanup** — see [cleanup_done_habits.py](../scripts/cleanup_done_habits.py).
   Drop habits.csv rows where `status == 'done'` AND
   `completed_date <= today - 7d`.
7a. **`last_touched` column + backfill** — if the
    column is missing it is added; rows whose `last_touched` is empty
    are backfilled from `created_date` (fallback: today). Migration
    rule that runs on every `--fix` invocation; idempotent after the
    initial add. Mutators (`add_task.py`, `defer_task.py`,
    `defer_habit.py`, `skip_habit.py`, `brain tasks complete`,
    `touch_task.py`, `backlog_task.py`, `set_linear_issue.py`) keep
    the column fresh by calling `_csvlib.touch_row()` on every row
    mutation; `apply_sync_rules.py --fix` does the same for rows it
    repairs.
8. **Habit spawn on completion** — handled by `brain tasks complete`,
   not the sync. When a habits.csv row flips to `done`, a new row is
   appended with a fresh `H###` `task_id`, `status = not_started`, and
   `due_date` computed with the native anchor-to-due recurrence logic:
   anchor to the **original due_date** plus N × interval, where N is
   the smallest integer that makes the result **strictly after today**.
   A stale Monday-weekly habit lands on the next future Monday;
   a daily habit completed today schedules tomorrow. LLMs are bad at
   calendar arithmetic — always use the binary command.

## Bidirectional task ↔ project link

9. **Forward link** — `tasks.csv:project` must point to an existing
   `~/brain/projects/<slug>/` directory.
10. **Reverse link** — every `projects/<slug>/.METADATA.json` has a
    `tasks: [task_id, ...]` array that must mirror the forward link.

The `--fix` mode patches the reverse direction (writes missing
`task_id`s into `.METADATA.json`). It never silently drops a CSV row,
never invents a project, never mutates the forward link — those need
the user. See [task-project-link.md](task-project-link.md).

## Warnings (logged, no mutation)

11. **Misplaced habit** — a tasks.csv row whose `task_type` contains
    `habit` should be moved to habits.csv.
12. **Sub-task scaffold in notes** — any `- [ ]` checkbox pattern in
    `notes` triggers a "consider /todo turn-into-project" hint.
    Golden rule: **no sub-tasks in tasks.csv**.

## Order of operations in `/second-brain reindex`

```
1. projects   (existing — projects-lookup.csv)
2. resources  (existing — zotero-lookup.csv)
3. tasks      (NEW — applies rules 5-10, runs habit cleanup, validates links)
```

Step 3 deliberately runs after projects so the project-folder list is
up to date for link validation.

## Reindex is not a write-by-default tool

Without `--fix`, reindex is read-only and reports issues. Production
runs (`/second-brain reindex`) call `--fix` automatically; ad-hoc CLI
invocations are dry-run by default so you can preview before mutating.
