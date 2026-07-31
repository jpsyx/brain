# /todo command catalog

The user phrases requests informally — match them to the actions
below. When in doubt, confirm before mutating.

## Referencing tasks: `<task>`

Anywhere a command takes `<task>`, the user may pass:

- A **short ID**: `T17` (case-insensitive, so `t17` works) for a row
  in tasks.csv, or `H42` (or `h42`) for a row in habits.csv.
- A **bare integer**: `17` resolves to either `T17` or `H17`. If both
  exist, the CLI errors and asks for the prefix.
- A **name fragment**: case-insensitive substring of `task_name`,
  e.g. `apple store`. If two rows match, the CLI lists them with
  their IDs and exits — pick by ID and rerun.

This is implemented in [`scripts/_csvlib.py`](../scripts/_csvlib.py)
`find_by_id_or_fuzzy()` + `locate()`. **Short IDs are shorthand;
name fragments still work.**

## Capture

- **`/todo add <freetext>`** — parse the freetext into a row. Required
  fields (ask only if missing): `task_name`, `task_type`, `priority`.
  Defaults: `due_date` empty unless mentioned, `hard_deadline=false`,
  `status=not_started`, `defer_count=0`, everything else empty.
  Implementation: [scripts/add_task.py](../scripts/add_task.py).

  **Chunked tasks** (`--chunks N --duration M`): split the task into
  N sequential sessions of M minutes each. Names become
  `<name> (i/N)`; chunks share `due_date`, `priority`, `task_type`,
  `project`, `hard_deadline`; chunk i+1 is `blocked_by` chunk i;
  `mit` goes on chunk 1 only (then migrates on `done`). Trigger from
  phrasings like "split into 5 chunks of 30 minutes", "chunk this
  into 4 sessions of 45 min", "break this into three 30-min blocks".
  See SKILL.md "Chunked tasks" for the full lifecycle (creation,
  MIT migration, agenda surfacing). `--chunks` requires N >= 2 and
  cannot be combined with `--habit`.
- **`/todo add-habit <freetext>`** — same, but routes to habits.csv.
  Required: `recur_interval` + `recur_unit`. No `defer_count`,
  `start_date`, `blocked_by`, or `task_type` on habits.
- **`/todo add-to-project <project> "<freetext>"`** — like `add` but
  with `project` pre-filled. See [task-project-link.md](task-project-link.md).
- **`/todo remove <task>`** — drop a row. Confirms first. Removes
  task_id from any linked project's `tasks[]` array too (run sync).

## State changes

- **`/todo done <task>`** — run `brain tasks complete <task>` to set
  `status=done`, `completed_date=today`, and `last_touched=today`.
  If the row is in habits.csv, the binary also appends the next occurrence.
- **`/todo defer <task> +Nd|YYYY-MM-DD`** — push `due_date`,
  increment `defer_count`. Warns at `defer_count >= 3`. See
  [scripts/defer_task.py](../scripts/defer_task.py).
  **Defer-demote rule (deterministic, in the script):** any defer
  strips `mit` from `task_type` and demotes `p0 → p1`. Lower
  priorities keep their level but still lose MIT. Rationale: if it
  can wait, it isn't urgent + critical anymore. Only the script
  enforces this — don't rely on the LLM to remember.
  **Chunked-task cascade (deterministic, in the script):** if the
  deferred row is a chunk, later chunks whose `due_date` would
  invert the family order are automatically pushed forward to the
  new date (no cascade if they're already later). Cascaded chunks
  get a fresh `last_touched` but their `defer_count`, `priority`,
  and `task_type` are untouched — only the explicitly deferred
  chunk is treated as "deferred". See SKILL.md "Chunked tasks →
  Defer cascade".
- **`/todo touch <task>`** — bump `last_touched` to today, no other
  changes. Use for the chronic-ignore "revive" action: the user
  acknowledges a stale task ("yes I still care"); it then has
  another 21 days before resurfacing in chronic-ignore. See
  [scripts/touch_task.py](../scripts/touch_task.py).
- **`/todo defer-habit <habit> [--occurrences N]`** — skip the next
  N occurrences of a habit (default 1). Advances `due_date` by N
  recurrence intervals using the same anchor-to-due-with-catch-up
  math as `brain tasks complete`, so a Monday-weekly habit stays on Mondays
  after skipping. Use this instead of `defer` for habits — raw
  `+Nd` would knock weekly/monthly cycles off-rhythm. No
  `completed_date` is recorded (the skipped instance is simply not
  done). See [scripts/defer_habit.py](../scripts/defer_habit.py).
- **`/todo priority <task> p0|p1|p2|p3|p4`**
- **`/todo mit <task>`** — adds `mit` to `task_type` (and removes if
  already present — toggle).
- **`/todo set <task> <field>=<value>`** — generic setter for
  `energy_level`, `context`, `estimated_duration`, `project`,
  `start_date`, `blocked_by`, `notes`, `see_also`, `hard_deadline`.

## Retrieval

- **`/todo today`** — due today + past-due + MITs. Hides
  `start_date > today`. Hides habits already completed today.
- **`/todo week`** — due in the next 7 days (inclusive).
- **`/todo past-due`** — anything overdue and not done.
- **`/todo list [filters...]`** — read the CSV directly and filter
  in Python (or eyeball it — the file is small). Examples:
  `priority<=p1`, `project=apply-to-ict4d-conference`,
  `energy=low`, `context=computer`, `type=code`.
- **`/todo search <query>`** — `rg -i` over `task_name` + `notes`
  across both CSVs.
- **`/todo habits`** — all active habit instances (status !=
  done). Useful before bed for "did I do everything?"

## Personal assistant

These are the load-bearing commands. The skill is a personal
assistant first.

- **`/todo what`** (alias: `/todo next`) — "what should I work on
  right now?" Priority order:
  1. Past-due hard deadlines (`hard_deadline=true` AND `past_due`)
  2. MITs due today or past-due
  3. Highest priority (p0 → p4) among visible-today
  4. Within same priority, shortest `estimated_duration` first
     (encourages quick wins).
  Filters: if user provides `energy=low` or `context=...`, apply
  before sorting. Ask the user for `estimated_duration` ONLY if it
  would change the top answer.
- **`/todo agenda [today|tomorrow|YYYY-MM-DD]`** — day briefing.
  Ordered list with cumulative durations. Hard-deadlines first,
  then MIT, then by priority, fitting durations into a typical
  workday. Markdown output.
  **Persistence:** every time you build or rework an agenda,
  **write it to `/tmp/<TARGET_DATE>.md`** (overwriting). When
  tasks on a persisted agenda are deferred or touched via the
  mutator scripts (`defer_task.py`, `defer_habit.py`, `touch_task.py`),
  those scripts auto-update the file via
  [scripts/update_agenda_on_mutation.py](../scripts/update_agenda_on_mutation.py)
  — see SKILL.md operating principle 7. For completions
  (`brain tasks complete <id>`) and non-mutation reworks (drops, swaps,
  manual reorderings), rewrite the file yourself.
  The user reads these files via the `agenda` zsh helper
  (`agenda today` / `agenda tomorrow` / `agenda YYYY-MM-DD` /
  bare `agenda` for the latest). Source:
  the `agenda` command.
- **`/todo plan-day`** — interactive variant of `agenda`. Walks user
  through ordering choices.
- **`/todo triage`** — see [triage-heuristics.md](triage-heuristics.md).
  Bulk-group past-due tasks; offer defer-all / drop-all / 1-by-1.
- **`/todo chronic`** — list chronically-ignored tasks via
  [scripts/find_chronic_ignored.py](../scripts/find_chronic_ignored.py).
  Same set that `/triage` Step 7 sweeps; useful for ad-hoc inspection
  without running a full triage pass. Pass `--count` for just the
  number; `--pretty` for human-readable JSON.

## Project linkage

See [task-project-link.md](task-project-link.md) for the full doc.

- **`/todo turn-into-project <task>`** — conversion workflow.
- **`/todo link <task> <project>`** — bidirectional link.
- **`/todo unlink <task>`** — clear forward + reverse links.
- **`/todo project-tasks <project>`** — list tasks for a project.
- **`/todo project-status <project>`** — done / total counts +
  ETA.
- **`/todo orphans`** — alias for `apply_sync_rules.py` dry-run.

## Linear linkage (code tasks)

See [linear-link.md](linear-link.md) for the full doc. The link is
LLM-mediated (scripts can't reach the Linear MCP); placement/structure
judgment is delegated to `/linear-pm`.

- **`/todo file-in-linear <task>`** — file a `code` task as a Linear
  issue. Hands placement (Backlog / project / cycle) + priority + labels
  to `/linear-pm`, confirms the draft (`AskUserQuestion`), creates the
  issue (`save_issue`), then persists the link via
  `set_linear_issue.py`. Confirm before any Linear write.
- **`/todo link-issue <task> <AVA-###> [url]`** — link an existing task
  to an existing issue. Wraps
  [scripts/set_linear_issue.py](../scripts/set_linear_issue.py).
- **`/todo unlink-issue <task>`** — clear the `linear_issue` link.
  `set_linear_issue.py <task> --clear`.
- **`/todo sync-linear`** — reconcile linked code tasks with Linear on
  demand (the same pass /triage runs daily):
  `list_linked_tasks.py --open-only` → `get_issue` each → sync **state
  and properties** (status, `due_date`↔`dueDate`, `priority`,
  `task_name`↔`title`; most-recently-edited wins, conflicts surfaced);
  apply the **ownership filter** (mirror in the user's untracked code
  issues via a **time-windowed** `list_issues` scan whose `updatedAt`
  window is derived from the last completed Morning Triage — never the
  whole workspace; drop ones reassigned to others); create
  **PR-review tasks** for PRs awaiting the user's review (deduped on PR URL).
  See [linear-link.md](linear-link.md) and
  [scripts/list_linked_tasks.py](../scripts/list_linked_tasks.py).

`/todo add` also accepts `--linear-issue AVA-###` to set the link at
creation (rare — the issue usually doesn't exist yet). `/todo done` on a
linked task prints a reminder to close the Linear issue too; `/todo
remove` should cancel it (never delete).

## Sync

- **`/todo sync`** — runs `apply_sync_rules.py --fix` +
  `cleanup_done_habits.py`. Same code paths invoked by
  `/second-brain reindex`.
- **`/second-brain reindex`** — runs the full brain reindex, including
  tasks. See `../second-brain/SKILL.md`.

## Natural-language matching cheat sheet

`<task>` below can be a short ID (`T17`, `H42`, or bare `17`) or a
name fragment.

| User says… | Map to |
|---|---|
| "remind me to X" / "add X" / "todo X" | `/todo add` |
| "split X into N chunks of M minutes" / "break X into N M-min sessions" / "chunk X" | `/todo add --chunks N --duration M` (see SKILL.md "Chunked tasks") |
| "I did T17" / "mark t17 done" / "finished apple store" | `/todo done` |
| "defer 17 to Friday" / "snooze T42" / "push the laundry task" | `/todo defer` |
| "skip workout tomorrow" / "not doing H29 today" / "defer that habit" | `/todo defer-habit` |
| "what should I do?" / "what's next?" | `/todo what` |
| "structure my day" / "plan my morning" | `/todo plan-day` |
| "anything slipping?" / "morning triage" | `/todo triage` |
| "what's been ignored?" / "what's rotting?" / "any deadwood?" | `/todo chronic` |
| "I still care about T17" / "leave it on the list" / "revive 17" | `/todo touch` |
| "what's on my plate?" / "show me today" | `/todo today` |
| "break X down" / "X should be a project" | `/todo turn-into-project` |
| "how's project Y going?" | `/todo project-status` |
| "add this to Linear" / "file X in Linear" / "make a Linear issue" | `/todo file-in-linear` |
| "link T17 to AVA-123" / "this is AVA-123" | `/todo link-issue` |
| "sync with Linear" / "did anything close in Linear?" | `/todo sync-linear` |
