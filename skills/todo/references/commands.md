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

This is implemented in `brain`
`find_by_id_or_fuzzy()` + `locate()`. **Short IDs are shorthand;
name fragments still work.**

These locators intentionally use mutable `task_id`, then update the matched
row without changing its immutable `task_uuid`.

## Capture

- **`/todo add <freetext>`** — parse the freetext into a row. Required
  fields (ask only if missing): `task_name`, `task_type`, `priority`.
  New rows receive a UUIDv4 `task_uuid`. Defaults: `due_date` empty unless mentioned, `hard_deadline=false`,
  `status=not_started`, `defer_count=0`, `assigned_to=BRAIN_ACTOR_ID`,
  everything else empty. `--assigned-to <user-id>` is an explicit override
  and must name a member in the selected workspace's `.config/users.json`.
  Implementation: `brain tasks add`.

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
- **`/todo remove <task>`**: preview explicit targets with
  `brain tasks remove T17 --dry-run --json`, obtain consent for the selected
  policies, then apply through the native CLI. Dependency, reference, and project
  link cleanup belong to the CLI. See [Remove tasks](../SKILL.md#remove-tasks).
  Submit all consented IDs in one command for a bulk drop.

## Native removal examples and recovery

These examples inherit the selected workspace and effective actor. Select a
workspace explicitly with `-w sample` when operating outside a Brain session.
After consent, a preview-bound apply uses that preview's fingerprint:

    brain -w sample tasks remove T17 --yes --expect-plan "$fingerprint" --json

`$fingerprint` and `$operation_id` are shell-variable templates: use the
actual preview fingerprint and reported operation ID, respectively.

    brain tasks remove H42 --habit --yes
    brain tasks remove T17 --linked-issue keep --yes
    brain -w sample tasks remove --operation-status "$operation_id" --json
    brain -w sample tasks remove --resume "$operation_id" --json
    brain -w sample tasks remove --abandon "$operation_id" --yes --json

Status and resume accept an operation ID instead of targets; do not add `--yes`,
preview, habit, or policy flags. Abandon requires explicit consent and `--yes`;
it cannot undo a committed deletion. A stale fingerprint requires a fresh preview
and consent. Do not retry a committed deletion because derived agenda output
reported a warning. Report the operation ID and inspect its saved status instead.
Purge and dedupe retain their specialized `brain backlog purge` and
`brain backlog dedupe` commands; do not reconstruct their candidate sets or
remove their rows by hand.

## State changes

- **`/todo done <task>`** — run `brain tasks complete <task>` to set
  `status=done`, `completed_date=today`, and `last_touched=today`.
  If the row is in habits.csv, the binary also appends the next occurrence.
- **`/todo defer <task> +Nd|YYYY-MM-DD`** — push `due_date`,
  increment `defer_count`. Warns at `defer_count >= 3`. See
  `brain tasks defer`.
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
  `brain tasks touch`.
- **`/todo defer-habit <habit> [--occurrences N]`** — skip the next
  N occurrences of a habit (default 1). Advances `due_date` by N
  recurrence intervals using the same anchor-to-due-with-catch-up
  math as `brain tasks complete`, so a Monday-weekly habit stays on Mondays
  after skipping. Use this instead of `defer` for habits — raw
  `+Nd` would knock weekly/monthly cycles off-rhythm. No
  `completed_date` is recorded (the skipped instance is simply not
  done). See `brain habits defer`.
- **`/todo priority <task> p0|p1|p2|p3|p4`**
- **`/todo mit <task>`** — adds `mit` to `task_type` (and removes if
  already present — toggle).
- **`/todo assign <task> <user-id>`** — explicit reassignment through
  `brain tasks assign`; validates portable membership and preserves every
  unrelated field.
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
  `energy=low`, `context=computer`, `type=code`. Shared workspaces also accept
  `assigned_to=<user-id>`; one-person workspaces hide that filter.
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
  **write it to `/tmp/<TARGET_DATE>.md`** (overwriting). Every task
  mutation keeps that file current on its own: `brain tasks complete`
  syncs it in-process, and the mutator scripts (`brain tasks defer`,
  `brain habits defer`, `brain tasks touch`, `brain backlog park`) hand it to
  `brain tasks sync-agenda` via
  `brain tasks sync-agenda`
  — see SKILL.md operating principle 7. Only **non-mutation** reworks
  (drops, swaps, manual reorderings) are yours to write.
  The user reads these files via the `agenda` zsh helper
  (`agenda today` / `agenda tomorrow` / `agenda YYYY-MM-DD` /
  bare `agenda` for the latest). Source:
  the `agenda` command.
- **`/todo plan-day`** — interactive variant of `agenda`. Walks user
  through ordering choices.
- **`/todo triage`** — see [triage-heuristics.md](triage-heuristics.md).
  Bulk-group past-due tasks; offer defer-all / drop-all / 1-by-1.
- **`/todo chronic`** — list chronically-ignored tasks via
  `brain tasks chronic`.
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
- **`/todo orphans`** — alias for `brain tasks lint` dry-run.

## External issue tracker (optional)

A task may carry an external tracker issue identifier in the
`linear_issue` column. Core treats it as inert local metadata: an empty
link is always fine. Filing issues, syncing state with the tracker, and
reconciling are the tracker workflow supplied by the `todo:linear`
extension when one is installed; its commands are documented there.

- **`/todo link-issue <task> <issue-id>`** — link an existing task to an
  existing issue. `brain tasks set <task> --linear-issue <issue-id>`.
- **`/todo unlink-issue <task>`** — clear the link.
  `brain tasks set <task> --linear-issue ""`.
- **`brain tasks linked`** — list linked tasks (`--open-only`, `--json`).

`/todo add` also accepts `--linear-issue <issue-id>` to set the link at
creation (rare — the issue usually doesn't exist yet). `/todo done` on a
linked task prints a reminder to close the linked issue too. `/todo remove`
uses the configured deterministic adapter for cancellation. Explicit local-only
removal uses `--linked-issue keep`; do not replace a failed adapter with an
agent-tool cancellation. See [Remove tasks](../SKILL.md#remove-tasks).

## Reindex

- **`/todo reindex`** — runs `brain tasks lint --fix` +
  `brain habits cleanup`. Same code paths invoked by
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
| "link T17 to ABC-123" / "this is ABC-123" | `/todo link-issue` |
