# Schema reference

Human-readable mirror of `~/brain/tasks/SCHEMA.json`. SCHEMA.json is
the machine source-of-truth; this file is for reading.

## Files

```
~/brain/tasks/
  tasks.csv      # non-habit tasks
  habits.csv     # recurring habits
  SCHEMA.json    # canonical schema (machine)
```

CSV conventions: snake_case column names, lowercase enum values,
no emojis. Multi-value cells use `|` separator. Dates in ISO 8601
(`YYYY-MM-DD`; add `THH:MM` only when time matters).

**Chunked-task naming:** rows in tasks.csv whose `task_name` ends in
`(i/N)` (e.g. `Draft whitepaper (2/5)`) are time-boxed chunks of a
single logical task. The trailing `(i/N)` fraction is the canonical
chunk marker and is parsed by
[`_csvlib.parse_chunk_name`](../scripts/_csvlib.py) and by the
native `brain tasks complete` path to migrate the `mit` tag forward
when a chunk completes. See
[SKILL.md "Chunked tasks"](../SKILL.md#chunked-tasks)
for the full lifecycle. Don't use this naming pattern for anything
other than chunks; otherwise the script-level chunk detection
misfires.

## Task IDs

Short, stable, human-typable. Two disjoint namespaces, one per CSV:

- **Tasks** (`tasks.csv`): `T1`, `T2`, `T3`, … (prefix `T`)
- **Habits** (`habits.csv`): `H1`, `H2`, `H3`, … (prefix `H`)

Issued by [`scripts/next_id.py`](../scripts/next_id.py); never reused, never
edited by hand. Counters: `~/brain/tasks/.tasks_next_id` and
`~/brain/tasks/.habits_next_id`. Each habit occurrence (spawned on `done`)
gets a fresh `H###`.

CLI/LLM input is forgiving: `T17`, `t17`, and bare `17` all resolve. A
bare integer that matches both a `T<n>` and an `H<n>` errors and asks
the user to disambiguate with the prefix.

**Any user-facing output that lists tasks MUST include the `task_id` so
the user can reference rows in follow-ups** ("done T42", "defer 17 +3d").

## tasks.csv columns

| # | Column | Type | Notes |
|---|---|---|---|
| 1 | `task_id` | short ID (`T###`) | Stable. Issued by `scripts/next_id.py`; never edited. Tasks use the `T` prefix (e.g. `T17`); habits use `H` (e.g. `H42`). |
| 2 | `task_name` | string | Short title. |
| 3 | `task_type` | enum-set, `\|`-sep | `ceo`, `aa`, `personal`, `code`, `languages`, `finance`, `mit`, `needs_attention`, `unassigned`. (`habit` is excluded — habits live in habits.csv.) |
| 4 | `status` | enum | `not_started`, `in_progress`, `waiting`, `done`, `backlog`. `backlog` = parked indefinitely (no `due_date`/`start_date`; `backlogged_date` set; hidden from all active views; auto-deleted >6mo). |
| 5 | `priority` | enum | `p0` (urgent) → `p4` (someday). |
| 6 | `due_date` | date \| datetime \| empty | Empty if no due date. |
| 7 | `hard_deadline` | bool | `true` / `false`. Triage doesn't bulk-defer these. |
| 8 | `start_date` | date \| empty | Hide from views until this date. |
| 9 | `assignee` | string | Default `me`; future-proofing if delegation comes in. |
| 10 | `see_also` | string | URL or freetext context. |
| 11 | `notes` | string | Task body. `- [ ]` patterns trigger "consider /todo turn-into-project". |
| 12 | `project` | kebab-slug \| empty | Forward link to `~/brain/projects/<slug>`. |
| 13 | `energy_level` | enum \| empty | `high`, `medium`, `low`. Empty until needed for an assistant decision. |
| 14 | `context` | enum \| empty | `home`, `office`, `computer`, `calls`, `errand`. Empty until needed. |
| 15 | `estimated_duration` | int (minutes) \| empty | Thresholds: 5/15/30/45/60+. Best-guess silently when confident, ask user only when it would change an assistant decision. |
| 16 | `blocked_by` | task_id list, `\|`-sep \| empty | Dependencies. |
| 17 | `defer_count` | int | Starts at 0. Increments on every defer. `>=3` triggers triage warning. |
| 18 | `created_date` | date | Auto-set on insert. |
| 19 | `completed_date` | date \| empty | Auto-set when `status` flips to `done`. |
| 20 | `last_touched` | date | Auto-bumped to today by every row mutator (`add_task.py`, `defer_task.py`, `defer_habit.py`, `brain habits skip`, `brain tasks complete`, `touch_task.py`, `backlog_task.py`, `set_linear_issue.py`, and `apply_sync_rules.py --fix`). Drives chronic-ignore detection for tasks and last-writer-wins CSV sync for both tasks and habits. Backfilled from `created_date` on migration. |
| 23 | `backlogged_date` | date \| empty | Set by `backlog_task.py` when a task enters `status=backlog`; cleared on restore. Drives the 6-month auto-purge (`purge_old_backlog.py`) and the monthly backlog-review. |

## habits.csv columns

Same as tasks.csv columns 1–15 + 18–20, plus:

| Column | Notes |
|---|---|
| `recur_interval` | int |
| `recur_unit` | `days` / `weeks` / `months` |

`defer_count`, `blocked_by`, `start_date`, and `task_type` are
omitted from habits.csv — habits don't get deferred (they recur)
and don't need a type (they're all "habit").

When a habit row is marked `done`, the system anchors to the
original `due_date` + `recur_interval × recur_unit`, then
fast-forwards by that interval until strictly past today.
A daily habit completed today schedules tomorrow; an 8-weeks-stale
Monday-weekly lands on the next future Monday.

## Derived columns (not stored)

Computed on read. See [sync-rules.md](sync-rules.md).

- `is_done` = `status == 'done'`
- `is_mit` = `'mit' in task_type`
- `past_due` = `due_date < today AND status != 'done'`
- `is_visible_today` = `(start_date == '' OR start_date <= today) AND status NOT IN ('done', 'backlog')`
- `is_stale` = `(today - last_touched) >= 21 AND status NOT IN ('done', 'backlog')`
- `is_backlogged` = `status == 'backlog'` (parked indefinitely; no `due_date`/`start_date`; `backlogged_date` set; auto-deleted >6mo)
- `is_stuck_in_progress` = `status == 'in_progress' AND (today - last_touched) >= 14`
- `is_captured_forgotten` = `(today - created_date) >= 60 AND status == 'not_started' AND notes == '' AND estimated_duration == '' AND project == ''`
- `is_chronic_ignore` = `(is_stale OR is_stuck_in_progress OR is_captured_forgotten) AND NOT past_due AND (due_date == '' OR due_date > today + 14)` — the set surfaced by [`scripts/find_chronic_ignored.py`](../scripts/find_chronic_ignored.py) and swept in `/triage` Step 7.

## Estimated-duration thresholds

| Label | Minutes |
|---|---|
| Quick | 5 |
| Short | 15 |
| Medium | 30 |
| Long | 45 |
| Very long | 60+ (90+ = should be a project) |

## Notion → CSV mapping (one-time migration)

For provenance. The initial dump on 2026-06-08 mapped:

- `Task name` → `task_name`
- `Task type` (multi-select with emojis) → `task_type` (snake_case,
  no emojis, pipe-sep). Habit-tagged rows routed to habits.csv.
- `Status` ("Not started" etc.) → `status` (snake_case).
- `Priority` (P0–P4) → `priority` (p0–p4).
- `Due date.start` → `due_date`.
- `Hard deadline?` `__YES__/__NO__` → `hard_deadline` `true/false`.
- `Recur Interval`, `Recur Unit` ("Day(s)" → "days") → habits only.
- `See also` → `see_also`.
- Page bodies (where non-default) → `notes`. Default scaffolds
  (empty Sub-tasks + Supporting files headers) were skipped.
- Page `createdTime` → `created_date`.
- `Assignee` was always self → `assignee = me`.
- `task_id` was originally generated as UUID4 and later migrated to short
  IDs (`T1..Tn` for tasks, `H1..Hn` for habits) on 2026-06-08 via
  `scripts/next_id.py`. Counters live at `~/brain/tasks/.tasks_next_id`
  and `~/brain/tasks/.habits_next_id`.
- `defer_count = 0`, `completed_date = ''`, project/energy/context/start/blocked_by all empty.
- `estimated_duration` was inferred from notes when a duration hint
  was present (e.g. "Likely duration: 30 mins" → 30); empty otherwise.

Notion is deprecated as of 2026-06-08. tasks.csv is the canonical
source going forward.
