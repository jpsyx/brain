# Task ↔ Project linkage

Tasks live in `~/brain/tasks/tasks.csv` (and habits.csv). Projects
live in `~/brain/projects/<slug>/`. The two systems are linked
**bidirectionally** and validated by [sync](sync-rules.md) — purely
in Python, no LLM judgment.

Both `/todo` and `/second-brain` read this file. Edit here, not in
either skill.

## Golden rule: no sub-tasks in tasks.csv

The moment a task would need sub-tasks, **it's a project**. A
`tasks.csv` row with `- [ ]` checkbox patterns in `notes` is a sync
warning, and `/todo` should offer to convert.

Side effects of the golden rule:

- One row in tasks.csv = one unit of work, atomic.
- Decomposition lives in a project's folder + its child tasks, never
  inside a task's `notes` field.

## Forward link

`tasks.csv:project` is a kebab-case slug matching
`~/brain/projects/<slug>/`. Empty = unassigned. Same applies to
habits.csv.

## Reverse link

`projects/<slug>/.METADATA.json` gains a new field:

```json
{
  "name": "apply-to-ict4d-conference",
  "title": "Apply to the ICT4D conference",
  "status": "in-progress",
  "priority": "p1",
  "due": "2026-07-15",
  "directory": "projects/apply-to-ict4d-conference",
  "tasks": ["T17", "T18", "H42"]
}
```

The `tasks` array lists every `task_id` (short ID, `T###` or
`H###`) currently linked to the project. Empty array = no tasks
linked.

Both directions must always agree. Sync validates and (with `--fix`)
patches the reverse direction.

## Validation (CLI, structured — never LLM)

[`apply_sync_rules.py`](../scripts/apply_sync_rules.py):

1. For every non-empty `tasks.csv:project`: assert the project
   folder exists. If not → log orphan, **never silently clear**.
2. For every `.METADATA.json:tasks[]` entry: assert the `task_id`
   exists in tasks.csv or habits.csv with matching `project`. If
   not → log orphan.
3. **Forward-but-not-reverse** (task points, project doesn't list):
   `--fix` writes the `task_id` into the project's `tasks[]`.
4. **Reverse-but-not-forward** (project lists, task doesn't point):
   logged only — could be a stale entry or a wrong task_id. User
   decides.

## Personal-assistant: when to convert a task to a project

`/todo` should proactively offer conversion (without the user
asking) when any of these triggers:

| Trigger | Why it suggests a project |
|---|---|
| `estimated_duration > 90` minutes | A 90+ minute task probably has internal structure. |
| `defer_count >= 3` | It's not getting done — likely too big to start. |
| Task name has scope verbs (`launch`, `build`, `migrate`, `ship`, `redesign`, `research`, `roll out`, `set up`) | Names like these almost always imply multiple steps. |
| User mentions sub-tasks ("first I need X, then Y, then Z") | The golden rule applies. |
| User explicitly asks ("break this down", "turn this into a project") | Direct trigger. |

## Conversion workflow: `/todo turn-into-project <task>`

1. **Confirm** the task to convert and propose a kebab-case
   **outcome-named** slug per [second-brain naming rules](https://...)
   (e.g. `apply-to-ict4d-conference`, not `ict4d-stuff`).
2. **Hand off to /second-brain** "Add a new project" workflow to
   create `projects/<slug>/.METADATA.json` + `README.md`. Ask the
   user for the project's `due`, starting `status`, and `priority`
   (`p0`–`p4`). For conversion specifically, a reasonable default
   for `priority` is the parent task's priority — but still confirm
   with the user rather than setting it silently.
3. **Propose sub-tasks** — 3 to 10 atomic tasks that together
   accomplish the project outcome. Each gets:
   - `task_name` (atomic, action-verb)
   - `priority` (default: same as parent task)
   - `due_date` (spread between today and the project's `due`)
   - `task_type` (default: same as parent task)
   - `estimated_duration` (best guess, see SKILL.md thresholds)
   - **`blocked_by`** — when a sub-task can't start until another
     sub-task finishes, set this to the blocking task's `task_id`
     (e.g. `T17`). Walk the proposed list and ask yourself: which
     of these have to happen in a specific order? Encode the
     dependencies explicitly. Don't leave the user to remember.
     Multiple blockers: comma-separated `T17,T18`.
   Show the proposed list **with the dependency chain rendered**
   (e.g. "T17 → T18 → T19" or a small ASCII tree if there are
   parallel paths). **Ask the user to confirm, edit, add, or
   remove.** Iterate until they approve.
4. **Write** the approved sub-tasks to tasks.csv. Every sub-task
   row MUST have:
   - `project=<slug>` (forward link).
   - `blocked_by` populated where applicable.
   Then populate `.METADATA.json:tasks[]` with all the sub-task
   `task_id`s (reverse link). Both directions must agree.
5. **Delete the original task row.** It's been subsumed by the
   project + its children.
6. **Run sync** so the lookup CSV mirrors the new project state
   and the bidirectional link validates clean.

**Every project conversion MUST end with:** sub-tasks in tasks.csv
(all linked via `project=<slug>`), reverse link in `.METADATA.json`,
and any sequential dependencies expressed via `blocked_by`. This
applies whether the conversion was triggered by `/todo
turn-into-project`, by `/triage` (daily or weekly), or by any other
flow.

## Other linkage commands

| Command | What it does |
|---|---|
| `/todo link <task> <project>` | Set `task.project = <project>`. Push `task_id` into project's `tasks[]`. Both sides written. |
| `/todo unlink <task>` | Clear `task.project`. Remove `task_id` from project's `tasks[]`. |
| `/todo add-to-project <project> "<text>"` | Create a new task already linked to `<project>` — same as `/todo add` with the `project` arg pre-filled. |
| `/todo project-tasks <project>` | List tasks for a project: open + done in last 7d. |
| `/todo project-status <project>` | Counts: done / total, MITs, past-due, ETA from `estimated_duration` sum. |
| `/todo orphans` | Run validation in dry-run mode; print orphan tasks + projects. Same as `apply_sync_rules.py` without `--fix`. |

## Archiving a project (extends `/second-brain` "Archive this project")

When archiving a project that still has linked tasks:

- For **open** tasks (status != done) — `/second-brain` asks the
  user three options per task:
  1. **Re-home** — move to a different project (`/todo link`).
  2. **Keep but unlink** — clear `task.project`, task becomes
     standalone.
  3. **Mark done** — task is finished as part of the archive.
- For **done** tasks — they stay in tasks.csv (or in habits.csv
  until the 7-day cleanup); their `project` field still points to
  the archived slug. That's fine — the archived project still
  exists, just under `archive/projects/<slug>/`. Sync handles the
  archive path automatically.

No silent task loss. No silent task orphaning.

## Reading the link from tasks → project

`/todo project-tasks <slug>` and `/todo project-status <slug>` read
`~/brain/tasks/tasks.csv` directly (it's small — ~150 rows) and
filter by `project == <slug>` and `status != done`, returning
`task_id, task_name, priority, due_date, defer_count`. Either read
the file and reason over it, or use a one-shot Python / `awk`
filter. No special tooling required.

## Reading the link from project → tasks

Read `projects/<slug>/.METADATA.json:tasks[]` and look each up in the
CSVs by `task_id`. No string-matching, no LLM disambiguation.
