# Task sync rules

Canonical reference for the automations that derive / validate /
mutate `<brain>/tasks/{tasks,habits}.csv`. Mirrors the four Notion
formulas (`Is Done`, `Is MIT`, `Past due`, `Next Due`) the old
Tasks Tracker used, plus the local-only rules this system adds.

Run via:

- `/todo reindex` - applies all rules to tasks.csv + habits.csv.
- `/organize reindex` - same, plus projects + zotero. See
  [organize reindex](../../organize/SKILL.md#reindex).
- `brain tasks lint [--fix]`
  - dry-run by default; pass `--fix` to write corrections.

## Derived (computed on read; not stored)

1. **`is_done`** - `status == 'done'`.
2. **`is_mit`** - `'mit'` in the pipe-split `task_type`.
3. **`past_due`** - `due_date < today` AND `status != 'done'`. Habits
   compute the same way; a missed habit is past-due until completed
   or until its next instance spawns.
4. **`is_visible_today`** - (`start_date == ''` OR `start_date <=
   today`) AND `status NOT IN ('done', 'backlog')`. Used to hide
   deferred tasks (future `start_date`), completed tasks, and parked
   `backlog` tasks from "Today" / "What should I work on" / "Agenda"
   views.
4a. **`is_stale`** - `(today - last_touched) >= 21` AND `status NOT IN
    ('done', 'backlog')`. Tasks-only even though habits also carry
    `last_touched`; habit recurrence controls their freshness.
    Backlog tasks can't be stale - they're parked on purpose.
4b. **`is_stuck_in_progress`** - `status == 'in_progress'` AND
    `(today - last_touched) >= 14`.
4c. **`is_captured_forgotten`** - `(today - created_date) >= 60` AND
    `status == 'not_started'` AND empty `notes`, `estimated_duration`,
    `project`.
4d. **`is_chronic_ignore`**: use `brain tasks chronic`. Native detection excludes
    done, backlog, future-start, and past-due tasks. It includes an undated task
    or one due from today through today + 3 days when any of the three age signals
    above applies. Do not copy an older fourteen-day exclusion rule.

## Mutations (`brain tasks lint --fix`)

5. **`completed_date` auto-set** - when `status == 'done'` and
   `completed_date` is empty, set to today.
6. **`defer_count` default** - empty → `0`.
7. **Habit cleanup** - see `brain habits cleanup`.
   Drop habits.csv rows where `status == 'done'` AND
   `completed_date <= today - 7d`.
7a. **`last_touched` column + backfill** - if the
    column is missing it is added; rows whose `last_touched` is empty
    are backfilled from `created_date` (fallback: today). Migration
    rule that runs on every `--fix` invocation; idempotent after the
    initial add. Mutators (`brain tasks add`, `brain tasks defer`,
    `brain habits defer`, `brain habits skip`, `brain tasks complete`,
    `brain tasks touch`, `brain backlog park`, `brain tasks set`) keep
    the column fresh through the native mutation implementation on every row
    mutation; `brain tasks lint --fix` does the same for rows it
    repairs.
8. **Habit spawn on completion** - handled by `brain tasks complete`,
   not the sync. When a habits.csv row flips to `done`, a new row is
   appended with a fresh `H###` `task_id`, `status = not_started`, and
   `due_date` computed with the native anchor-to-due recurrence logic:
   anchor to the **original due_date** plus N × interval, where N is
   the smallest integer that makes the result **strictly after today**.
   A stale Monday-weekly habit lands on the next future Monday;
   a daily habit completed today schedules tomorrow. LLMs are bad at
   calendar arithmetic - always use the binary command.

## Bidirectional task ↔ project link

9. **Forward link** - `tasks.csv:project` must point to an existing
   `<brain>/projects/<slug>/` directory.
10. **Reverse link** - every `projects/<slug>/.METADATA.json` has a
    `tasks: [task_id, ...]` array that must mirror the forward link.

The `--fix` mode patches the reverse direction (writes missing
`task_id`s into `.METADATA.json`). It never silently drops a CSV row,
never invents a project, never mutates the forward link - those need
the user. See [task-project-link.md](../../projects/references/task-project-link.md).

## Warnings (logged, no mutation)

11. **Misplaced habit** - a tasks.csv row whose `task_type` contains
    `habit` should be moved to habits.csv.
12. **Sub-task scaffold in notes** - any `- [ ]` checkbox pattern in
    `notes` triggers a "consider /todo turn-into-project" hint.
    Golden rule: **no sub-tasks in tasks.csv**.

<a id="order-of-operations-in-second-brain-reindex"></a>

## Order of operations in `/organize reindex`

```
1. projects   (existing - projects-lookup.csv)
2. resources  (existing - zotero-lookup.csv)
3. tasks      (NEW - applies rules 5-10, runs habit cleanup, validates links)
```

Step 3 deliberately runs after projects so the project-folder list is
up to date for link validation.

## Native reindex behavior

`brain tasks lint` is read-only unless `--fix` is supplied. `brain reindex`
is a requested maintenance mutation; `brain reindex --tasks` applies task/habit
rules only. `/todo reindex` uses that native scoped command, while `/organize reindex`
owns broader local indexes. Neither command performs cloud synchronization.
