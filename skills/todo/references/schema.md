# Schema reference

Human-readable mirror of `<selected-workspace>/tasks/SCHEMA.json`. SCHEMA.json is
the machine source-of-truth; this file is for reading.

## Files

```
<selected-workspace>/tasks/
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
brain natively and by the
native `brain tasks complete` path to migrate the `mit` tag forward
when a chunk completes. See
[SKILL.md "Chunked tasks"](../SKILL.md#chunked-tasks)
for the full lifecycle. Don't use this naming pattern for anything
other than chunks; otherwise the script-level chunk detection
misfires.

## Task identity

`task_uuid` is the immutable UUID merge identity and the first column in the
current portable schema. It never changes when a row is edited, completed, or
assigned a different display ID. Legacy rows receive a deterministic UUIDv5 from
`<workspace-uuid>:<csv-kind>:<legacy-task-id>` during the coordinated schema
migration. Newly created rows and spawned habit occurrences receive UUIDv4.

`task_id` is a short, mutable, human-facing display identity. It remains the
value users type and the value command locators accept:

- **Tasks** (`tasks.csv`): `T1`, `T2`, `T3`, … (prefix `T`)
- **Habits** (`habits.csv`): `H1`, `H2`, `H3`, … (prefix `H`)

Issued by `brain tasks add` and never reused.
Counters live beside each selected workspace's CSVs. Deterministic sync
reconciliation may change a display ID without changing `task_uuid`. Each
habit occurrence spawned on `done` gets both a fresh `H###` and a fresh UUID.

CLI/LLM input is forgiving: `T17`, `t17`, and bare `17` all resolve. A
bare integer that matches both a `T<n>` and an `H<n>` errors and asks
the user to disambiguate with the prefix.

**Any user-facing output that lists tasks MUST include the `task_id` so
the user can reference rows in follow-ups** ("done T42", "defer 17 +3d").

## tasks.csv columns

| # | Column | Type | Notes |
|---|---|---|---|
| 1 | `task_uuid` | UUID | Immutable merge identity. UUIDv4 for new rows; deterministic UUIDv5 for migrated legacy rows. |
| 2 | `task_id` | short display ID (`T###`) | Mutable human-facing identity. Tasks use `T`; habits use `H`. User commands locate by this value. |
| 3 | `task_name` | string | Short title. |
| 4 | `task_type` | enum-set, `\|`-sep | `ceo`, `aa`, `personal`, `code`, `languages`, `finance`, `mit`, `needs_attention`, `unassigned`. (`habit` is excluded because habits live in habits.csv.) |
| 5 | `status` | enum | `not_started`, `in_progress`, `waiting`, `done`, `backlog`. `backlog` = parked indefinitely (no `due_date`/`start_date`; `backlogged_date` set; hidden from all active views; auto-deleted >6mo). |
| 6 | `priority` | enum | `p0` (urgent) → `p4` (someday). |
| 7 | `due_date` | date \| datetime \| empty | Empty if no due date. |
| 8 | `hard_deadline` | bool | `true` / `false`. Triage doesn't bulk-defer these. |
| 9 | `start_date` | date \| empty | Hide from views until this date. |
| 10 | `assigned_to` | portable user ID | Defaults to the effective actor. Explicit values and reassignments must name a member in `.config/users.json`; unrelated edits preserve it. Readers temporarily accept legacy `assignee`, while writers emit only `assigned_to`. |
| 11 | `see_also` | string | URL or freetext context. |
| 12 | `notes` | string | Task body. `- [ ]` patterns trigger "consider /todo turn-into-project". |
| 13 | `project` | kebab-slug \| empty | Forward link to `<selected-workspace>/projects/<slug>`. |
| 14 | `energy_level` | enum \| empty | `high`, `medium`, `low`. Empty until needed for an assistant decision. |
| 15 | `context` | enum \| empty | `home`, `office`, `computer`, `calls`, `errand`. Empty until needed. |
| 16 | `estimated_duration` | int (minutes) \| empty | Thresholds: 5/15/30/45/60+. Best-guess silently when confident, ask user only when it would change an assistant decision. |
| 17 | `blocked_by` | task_id list, `\|`-sep \| empty | Dependencies. |
| 18 | `defer_count` | int | Starts at 0. Increments on every defer. `>=3` triggers triage warning. |
| 19 | `created_date` | date | Auto-set on insert. |
| 20 | `completed_date` | date \| empty | Auto-set when `status` flips to `done`. |
| 21 | `last_touched` | date | Auto-bumped to today by every row mutator (`brain tasks add`, `brain tasks defer`, `brain habits defer`, `brain habits skip`, `brain tasks complete`, `brain tasks touch`, `brain backlog park`, `brain tasks set`, and `brain tasks lint --fix`). Drives chronic-ignore detection for tasks and last-writer-wins CSV sync for both tasks and habits. Backfilled from `created_date` on migration. |
| 24 | `backlogged_date` | date \| empty | Set by `brain backlog park` when a task enters `status=backlog`; cleared on restore. Drives the 6-month auto-purge (`brain backlog purge`) and the monthly backlog-review. |
| 25 | `system_key` | string \| empty | Stable identity for a Brain-managed definition. Ordinary rows leave it blank; habit recurrence retains it. |

## habits.csv columns

Habits share `task_uuid`, `task_id`, assignment, lifecycle, scheduling,
description, and `system_key` columns with tasks, plus:

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
- `is_chronic_ignore` = `(is_stale OR is_stuck_in_progress OR is_captured_forgotten) AND NOT past_due AND (due_date == '' OR due_date > today + 14)` — the set surfaced by `brain tasks chronic` and swept in `/triage` Step 7.

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
- `Assignee` was mapped into the portable `assigned_to` field during writer migration.
- `task_id` was originally generated as UUID4 and later migrated to short
  IDs (`T1..Tn` for tasks, `H1..Hn` for habits) on 2026-06-08 via
  `brain tasks add`. Counters live beside the selected workspace's CSVs.
- `defer_count = 0`, `completed_date = ''`, project/energy/context/start/blocked_by all empty.
- `estimated_duration` was inferred from notes when a duration hint
  was present (e.g. "Likely duration: 30 mins" → 30); empty otherwise.

Notion is deprecated as of 2026-06-08. tasks.csv is the canonical
source going forward.
