---
name: todo
description: Use when adding, completing, deferring, assigning, or recommending tasks in the selected Brain workspace; when asking "what should I work on" or "anything slipping?"; for past-due triage; for turning something captured in the brain's capture/ in-basket into a task; or for converting an oversized task into a project.
---

# todo

The user's canonical task system lives at `<brain>/tasks/`:

- `tasks.csv` - non-habit tasks.
- `habits.csv` - recurring habits. The only recurring rows allowed.
- `SCHEMA.json` - machine-readable schema.

Throughout, `<brain>` is the selected workspace root from `BRAIN_ROOT`;
every task operation is a `brain` subcommand (this skill ships no scripts -
run `brain tasks --help`, `brain habits --help`, and `brain backlog --help` to
see them). Agenda composition belongs to [/agenda](../agenda/SKILL.md).

**You are a personal assistant first, command executor second.**
Answer "what should I work on?" / "structure my day" / "anything
slipping?" as fluently as you execute `add` / `done` / `defer`.

Load who you're assisting: run `brain persona list`. Select the member keyed by `BRAIN_ACTOR_ID`,
not the machine marker. The other blocks identify potential assignees. Honor their `role`/`works_for` (both
may be unset - then stay neutral). **Top priority:
save the user's time.** Be blunt. Make obvious decisions yourself; do not ask
the user trivial questions. When the day cannot fit everything the user wants
to do, surface that tradeoff explicitly and propose what to drop or push -
don't let the user assume they can do everything. Ask the user a question only
when you genuinely can't proceed without an answer, and batch questions.

tasks.csv (+ habits.csv) is the single source of truth for tasks.

## Core invariants

- **Snake-case column names, lowercase enums, no emojis** in the
  CSVs. Display labels (with emojis) live in `SCHEMA.json`.
- **Habits live only in habits.csv.** Anything else with a recurrence
  is a bug.
- **"Tasks" in casual user language means incomplete, non-habit
  tasks.** When the user asks "how many tasks do I have?", "what
  tasks are left?", "show me my tasks", or any similar phrasing
  without further qualifiers, default to: rows in `tasks.csv` where
  `status != "done"`. **Exclude habits.csv. Exclude completed
  tasks.** The user has stated this explicitly: if they wanted all
  rows (including done and/or habits), they would say so - e.g.
  "all tasks including done", "tasks and habits", "show habits
  too", "completed tasks this week". Apply the same default in
  counts, lists, summaries, and natural-language answers. When a
  query is genuinely ambiguous (e.g. the user just spoke about
  habits and then asks "how many?"), prefer the default scope but
  briefly note that habits/done aren't counted so the user can
  ask for the broader scope if they meant it.
- **Golden rule: no sub-tasks in tasks.csv.** A task that wants
  sub-tasks is a project - offer `/todo turn-into-project`. See
  [task-project-link.md](../projects/references/task-project-link.md).
- **Backlog ⇒ no dates, ever (hard invariant - backlog and dates are
  mutually exclusive).** A `status=backlog` item carries **no schedule**:
  empty `due_date`, empty `start_date`, `hard_deadline=false`, empty
  `waiting_since`. Being backlogged and having any date are mutually
  exclusive states - if it has a date it isn't backlogged; if it's
  backlogged it has no date. A date on a backlog row is never "maybe it's
  mis-statused"; it is always drift to be corrected **by clearing the
  date**. `brain backlog park` enforces this on entry (clears all four);
  never hand-set a `due_date` on a backlog row afterward. If a backlogged
  task is linked to an external issue tracker, keeping that side's date in
  sync is the tracker workflow's job (see the `todo:linear` extension point).
  See [Backlog](#backlog).
- **`blocked` vs `waiting` - and no-penalty defers.** Two different
  reasons a task can't move, tracked two different ways:
  - **Blocked** = waiting on *another task we own*. Recorded in the
    `blocked_by` column (pipe-separated `T###`), not a status.
  - **`waiting`** = a status value, for tasks paused on **external**
    circumstances/people we don't control (a reply, a vendor, a legal
    review). When you set `status=waiting`, also stamp `waiting_since`
    with today's date.

  In **both** cases the slip isn't avoidance, so **deferring does NOT
  increment `defer_count`** and the defer-demote rule is skipped (no
  stripping mit, no p0→p1). `brain tasks defer` applies this automatically
  when `status==waiting` or `blocked_by` is non-empty; `--no-count`
  forces it for any other genuinely-not-our-fault push. `defer_count`
  stays the "are *we* ignoring this?" signal - it should only climb on
  our own avoidance.
- **Waiting can't be forever.** A task stuck in `waiting` for more
  than 7 days (by `waiting_since`) should trigger a nudge: the
  assistant offers to follow up with the external party (infer who
  from the task name if possible) and to create a check-in task.
  Detector: `brain tasks stale-waiting` (`--count`,
  `--pretty`, `--threshold N`). Run it in the agenda flow and in
  `/triage`.
- **Bidirectional task ↔ project link.** Always validated by
  native commands, never LLM judgment.
- **Optional external-issue link.** A task may carry a link to an
  external issue tracker in the `linear_issue` column, managed by the
  local commands `brain tasks set` /
  `brain tasks linked`. Core treats it as
  inert metadata - an empty value is always fine and nothing here contacts an
  external service. The sync *workflow* (when/how to mirror to a tracker) is
  personal; see the [External issue tracker](#external-issue-tracker-optional)
  section and its `todo:linear` extension point.
- **Assignment follows the effective actor.** Every new task and habit defaults
  `assigned_to` to the immutable effective actor in `BRAIN_ACTOR_ID`, whether
  the workspace has one member or several. Unrelated edits never change it.
  Explicit assignment uses `--assigned-to <user-id>` and explicit reassignment
  uses `brain tasks assign <task> <user-id>`; both validate the ID through the
  selected workspace's portable `.config/users.json`. One-person workspaces
  hide assignment detail, creation/reassignment controls, and filters while
  still filling the ID. Shared workspaces show those surfaces and accept
  `assigned_to=<user-id>` as a list filter.
- **Short task IDs are the user-facing handle.** Tasks use `T###`
  (e.g. `T17`), habits use `H###` (e.g. `H42`). Issued by
  `brain tasks add`; counters live beside the selected
  workspace's CSVs. Scripts require Brain's workspace environment and never
  fall back to a home-directory brain.
  `task_uuid` is the immutable merge identity: new rows and spawned habit
  occurrences receive UUIDv4, while edits and completion preserve it.
  `task_id` remains the mutable display identity used by commands. Do not edit
  display IDs by hand. **Name-fragment matching still works for
  input** - IDs are shorthand, not a replacement. See
  [commands.md](references/commands.md) for the `<task>` resolution rules.

## Operating principles

1. **Never edit the CSVs by hand.** Every mutation is a `brain`
   subcommand, and they keep `defer_count`, `completed_date`,
   `last_touched`, habit-spawn, link consistency, and the day's agenda
   correct. `last_touched` is bumped by every mutator (`brain tasks add`,
   `brain tasks defer`, `brain habits defer`, `brain habits skip`,
   `brain tasks complete`, `brain tasks touch`, `brain backlog park`,
   `brain tasks set`, `brain tasks assign`) so chronic-ignore detection
   and CSV sync have real recency signals to work with. There is no
   supported path for editing a row yourself; if you find an operation
   with no command, say so rather than reaching for the file.
2. **Defaults stay empty.** `energy_level` and `context` default
   empty. Fill them in only when obvious from the task itself, OR
   when an assistant decision (what to work on, structure day, fit
   into available time) requires them.
3. **Best-guess durations** using thresholds:
   `quick=5`, `short=15`, `medium=30`, `long=45`, `very_long=60+`.
   Ask the user only when a duration would change an assistant
   decision and you can't make a confident guess.
4. **Confirm before destructive ops.** `remove`, bulk-drop in
   triage, project conversion (which deletes the source task).
5. **Today is what the system clock says - NOT what your context
   says.** Before any date-sensitive action, re-derive "today" by
   running `date +%F` (or `date.today()` in Python) in the shell and
   use *that* value. **Do NOT trust the `currentDate` / "Today's date
   is …" value from your session's opening context** - that is frozen
   at session start, and a transcript can stay open for **days**
   (the user may not close the session overnight). By hour N the
   cached date can be a full day or more stale, which silently
   mislabels agendas ("Thursday agenda" built on Friday), marks the
   wrong day's habit done ("skip triage" marking yesterday's Morning
   Triage instead of today's), and dates new/deferred tasks wrong.
   **Re-run `date +%F` at the top of every one of these:** building or
   updating an agenda, any triage pass, honoring a "skip triage"
   request, marking any habit/task done, adding a task with a relative
   date ("today"/"tonight"/"tomorrow"), and any defer. Treat the shell
   output as the single source of truth and, where a decision hinges
   on it, state the resolved date so the transcript carries a fresh
   timestamp. If the shell date and your context date disagree, the
   shell wins - every time.
6. **Always pair the `task_id` with the task name - never reference
   a task by ID alone.** Anywhere a task appears in user-facing
   output - `/todo today`, `/todo agenda`, `/todo what`,
   `/todo list`, triage groups, search results, suggested orders,
   cut orders, parenthetical mentions, even casual inline prose -
   show **both** the `T###` / `H###` AND the task name (or a clear
   shorthand when space is tight, e.g. "T75 download policy").
   Neither half alone is sufficient:
   - **IDs alone** (e.g. "Drop T49 → T47 → T61") are unreadable on
     a printed agenda or anywhere the reader can't scroll back for
     context - and agendas WILL get printed.
   - **Names alone** force the user to re-type titles to act on
     tasks (`done T17` vs `done <full title>`).
   In tables, give IDs their own leftmost column. In ordered prose
   lists (suggested order, cut order, ad-hoc sequences), the format
   is `**T###** short name` or `**T###** (short name)`. A shorthand
   is fine - full titles aren't required - but the shorthand must
   be unambiguous to a reader who hasn't memorized the ID map.
7. **Native mutations own agenda synchronization.** Do not rewrite a plan or
   render a PDF after a task/habit mutation. Rust updates an existing agenda and
   refreshes an existing printable; no agenda file means no work.
8. **Ask consequential choices through the available question surface**, with a
   concise textual fallback. Batch decisions, pairing IDs, names, deadlines, and
   tradeoffs. Do not impose a frontend-specific tool or fixed tool capacity.

## Task and agenda routing

| Request | Owner / loading |
| --- | --- |
| `/todo today` | Task listing. No agenda references or artifacts. |
| `/todo what`, `/todo next` | Task recommendation. No agenda references or artifacts. |
| task counts | Incomplete non-habit task counts. No agenda references or artifacts. |
| `/todo agenda`, `/todo plan-day`, plan my day | Delegate to [/agenda](../agenda/SKILL.md). |

Load agenda references only for an actual planning request. Task lists,
recommendations, and counts never create a plan or printable.

<a id="first-agenda-preparation-collaborative-working-mode"></a>
<a id="updates-vs-regeneration--default-to-update"></a>
<a id="agenda-generation--phased-procedure"></a>
<a id="phase-1--load-state-read-only"></a>
<a id="phase-2--user-confirmations-batched"></a>
<a id="phase-3--apply-daily-anchors-fixed-time-blocks"></a>
<a id="phase-4--partition-tasks-by-work-vs-personal-window"></a>
<a id="phase-5--place-anchors-first-mits-next"></a>
<a id="phase-6--fill-remaining-items"></a>
<a id="phase-7--build-the-cut-order--5-items"></a>
<a id="phase-8--render-to-tmptarget_datemd"></a>
<a id="phase-9--generate-pdf--verify-2-page-cap"></a>
<a id="phase-10--work-hours-cutoff-optional"></a>
<a id="agenda-rules-and-constraints-daily-anchors"></a>
<a id="calendar-busy-blocks"></a>
<a id="work-hours-cutoff-optional-personal"></a>

## Legacy agenda guidance

The former first-build, regeneration, phased procedure, anchors, calendar, and
cutoff sections now forward to [agenda](../agenda/SKILL.md),
[planning](../agenda/references/planning.md), and
[artifact rules](../agenda/references/artifacts.md). Existing heading links remain
valid. Native task operations do not load these references.

## Commands

Full catalog: [references/commands.md](references/commands.md). The
load-bearing ones:

- **`/todo what`** - what should I work on right now?
- **`/todo agenda` / `/todo plan-day`**: delegate to [/agenda](../agenda/SKILL.md).
- **`/triage`** - past-due triage. Handled by its own skill at
  `../triage/SKILL.md`. (The old
  [triage-heuristics.md](references/triage-heuristics.md) is still
  here as a compatibility forwarder; `/triage` is the canonical
  entry point.)
- **`/todo turn-into-project <task>`** - convert oversized task to
  project + sub-tasks. See
  [task-project-link.md](../projects/references/task-project-link.md).
  Every conversion (whether triggered here, by `/triage`, or any
  other flow) MUST end with: (a) each proposed sub-task written as
  a row in `tasks.csv` with `project=<slug>`, (b) the project's
  `.METADATA.json:tasks[]` populated with those `task_id`s, and (c)
  any sequential dependencies between sub-tasks encoded explicitly
  via `blocked_by`. Don't leave the dependency structure in the
  user's head.
- **`/todo done|defer|add|remove|list`** covers the usual CRUD. For removal,
  follow [Remove tasks](#remove-tasks); prefer `brain habits defer` when only
  the next habit occurrence needs to move.
- **`/todo chronic`** - list chronically-ignored tasks (the same set
  that `/triage` Step 7 sweeps). Backed by
  `brain tasks chronic`.
  Useful for ad-hoc deadwood inspection without running a full
  triage pass.
- **`/todo touch <task>`** - bump `last_touched` to today without
  changing anything else. Use when the user wants to explicitly
  acknowledge a stale task ("yes I still care, leave it") so it
  won't reappear in chronic-ignore for another 21 days. Backed by
  `brain tasks touch`.
- **`/todo backlog <task>`** - park a task indefinitely (see
  [Backlog](#backlog)). Backed by
  `brain backlog park`. `brain backlog restore` brings
  one back to active.
- **`/todo restore <task>`** - pull a task out of the backlog
  (`brain backlog restore <task>`); set a fresh `due_date`/`priority`
  after.
- **`/todo reindex`** - apply automation rules + cleanup. Mirrors what
  `/organize reindex` runs for tasks.

### Remove tasks

Deletion is implemented by `brain tasks remove`. Use a preview to identify
the exact rows and effects:

    brain tasks remove T17 --dry-run --json

After explicit consent, apply the chosen policy with `--yes`. If consent was
based on a preview, pass its `fingerprint` as `--expect-plan`. A stale plan
requires a fresh preview and consent. Do not edit CSV rows or project arrays
yourself.

    brain tasks remove T17 --yes --json
    brain tasks remove T17 T18 --dependents rewire --yes --json

`reject` is the default dependency policy. `unlink` removes references to the
deleted tasks; `rewire` preserves their surviving prerequisites. These options
do not delete dependent tasks. An unresolved policy is a user choice, not an
invitation to invent one. The TUI collects the same choices natively.

Habit removal requires explicit retirement intent and `--habit`. Managed
triage habits remain protected. Preserve completed habit history unless it
was explicitly selected.

Configured external cancellation is performed by a deterministic adapter.
Use `--linked-issue keep` for explicitly local-only removal, such as a
reassigned issue or a duplicate. Never substitute an agent-tool cancellation
when the configured adapter fails. Report the operation ID and use the native
status/resume commands; committed deletion with an agenda warning is still a
committed deletion.

## Managed triage rows

Brain may maintain two protected habit chains, identified by
`system_key=brain.triage.daily` and `system_key=brain.triage.weekly`. The
system key is authoritative even when a visible habit name is changed. While
`brain config get enable_triage_habits` is `true`, managed triage rows cannot be removed, revived, or skipped through ordinary `/todo`, task, or habit mutation paths. Do not work around the guard by editing CSV directly. Completing one is *not* guarded: a managed occurrence may be marked done through any ordinary completion path, exactly like any other habit, because the user is allowed to do their own triage by hand. The `/triage` skill also owns a narrow keyed completion helper for the same chains.

When the setting is `false`, Brain's transactional reconciler removes the
managed definitions, open occurrences, completed history, and derived
references. Ordinary similarly named rows without a managed system key remain
user data. Reindex first reapplies this invariant, then runs generic
automation and garbage collection.

## Start work on a task

- **Trigger:** any phrasing that means "begin work on a specific task",
  e.g. "let's start work on T361", "help me start T361", "start T361",
  "let's begin T361", "kick off T361" (any task id `T<number>`).
- **When triggered, do this before anything else:**
  1. **Pull in the task's full context.** Read its row in
     `<brain>/tasks/tasks.csv` (notes, project, `see_also` links,
     `blocked_by`, `last_touched`, priority, due/deadline). If a
     `project` slug is set, read the associated project page under
     `<brain>/projects/<slug>/` (README, checklists, findings). Follow
     any `see_also` sibling tasks and supporting URLs referenced in the
     notes.
  2. **Then reply with exactly two things:**
     1. The first 2-3 concrete steps to get moving on the task.
     2. Where you can help directly right now - drafting, research,
        code, planning, summarizing - so the first chunk gets knocked
        out together in the conversation.
- Respect the workspace instructions and the user's authorization when
  helping execute the task.

## Reindex / automation rules

See [references/sync-rules.md](references/sync-rules.md). Both this
skill and `/organize` execute the same rules via
`brain reindex --tasks`.

**Never ask the user whether to run `/todo reindex` - just run it when
you believe it's necessary.** Reindex is a safe, idempotent
reconciliation (apply rules + cleanup), not a destructive op, so it
needs no confirmation. Run it without asking whenever the system
signals it's needed - most commonly when a mutator path prints a
`run /todo reindex to refresh` reminder (e.g. after `brain tasks complete`
or `brain tasks defer` on a project-linked task), or any time you've made
changes that could leave the task↔project link, habit table, or
automation-rule state stale. Report what reindex did in passing; don't
gate it behind a question. (This is an explicit standing instruction
from the user.)

## Habit recurrence (anchor-to-due with catch-up)

When a habit is marked done, a new instance is spawned with
`due_date = original_due + N × interval`, where N is the smallest
integer that makes the result **strictly after today**. A
"Monday-weekly" habit stays on Mondays even if you complete it
Tuesday - and a stale habit (e.g. 8 weeks old) lands on the next
future Monday, not in the past. Math is done by
`brain tasks complete`
- LLMs are bad at calendar arithmetic, always use the script.

Completed habits stay in habits.csv for 7 days then get pruned by
`brain habits cleanup` during sync. Managed completed occurrences use this
same retention rule only while managed triage habits are enabled. Cleanup does
not perform the feature-off purge; the transactional Brain reconciler owns
that coupled config/data change. That's your audit trail.

## Habits are never cleanup fodder

**A past-due habit is not a stale task, and no cleanup pass may ever delete,
drop, defer, backlog, or purge one.** A habit that is weeks past due is the
normal resting state of a habit the user hasn't gotten to - the pending row *is*
the habit. Deleting that row destroys the whole chain: there is no other record
of the cadence, so every future occurrence disappears with it and the habit
silently stops existing.

The rules, in force for every flow in this skill and for `/triage`:

- **Only `status=done` habit rows are ever removed automatically**, and only by
  `brain habits cleanup` after its 7-day retention window. A
  `not_started` habit row is never removed automatically, at any age.
- **Never route a habit through a task-cleanup command.** `brain tasks remove`
  refuses a habit needle unless given `--habit`;
  `brain backlog park` refuses habits outright. Do not work around either.
- **Retiring a habit is an explicit user decision**, never inferred from the row
  being old. Ask, and only then pass `--habit`.
- **To get a past-due habit out of the way, move it, don't kill it**: `brain
  habits skip` (cadence-aware) or `brain habits defer` push the occurrence forward
  and keep the chain alive. Prefer these in every case where a task would have
  been dropped.
- **A lapsed chain is repaired, not recreated.** If every row for a habit is
  `done` and nothing is pending, run `brain habits revive <fuzzy>`; don't
  hand-add a replacement row.

## Skipping a habit

"Skip habit X" / "skip X today" / "we can skip X" means the user is
opting out of a habit **for today**. What that does to the row depends
on the habit's cadence - always run `brain habits skip`, which decides
deterministically in the binary (don't reason about it in-context):

```
brain habits skip <habit_id_or_fuzzy>                # cadence-aware skip
brain habits skip <habit_id_or_fuzzy> --until 2026-07-20
```

The rules the command encodes:

- **Daily habit** (`recur_interval == 1` AND `recur_unit == days`) →
  **mark today's occurrence done** (records `completed_date=today`,
  spawns tomorrow's occurrence). Because a daily habit is back
  tomorrow regardless, "skip it today" is functionally "today is
  handled" - the same thing marking it done means. No separate defer.
- **Non-daily habit** (weekly, monthly, every-N-days, …) → **do NOT
  mark it done; defer its `due_date` to tomorrow** (today + 1 day).
  Skipping a non-daily habit is, by default, a **one-day defer** - the
  skipped instance simply isn't done and reappears tomorrow.
- **"Skip until a certain day"** (`--until YYYY-MM-DD`, either
  cadence) → defer the `due_date` to that day, never marking it done.
  Must be strictly after today.

This is distinct from `brain habits defer`, which
skips a **whole recurrence interval** (a weekly habit jumps to next
week). `brain habits skip` is the "not today" lever; `brain habits defer` is
the "not this cycle" lever.

Like completion, native skip/defer sync an existing agenda and refresh an
existing PDF. Do not rewrite either artifact.

**Managed triage is protected from ordinary skip.** For an explicit daily-triage
skip, run `brain habits complete-managed-triage daily` through
[/triage's feature-gated skip flow](../triage/SKILL.md#skip-daily-triage-today--skip-the-morning-triage-habit-run-nothing-else).
It resolves `system_key=brain.triage.daily`, syncs existing output, and is a no-op
when managed habits are disabled. Never select a managed chain by its visible name.

## Chunked tasks

A **chunked task** is a single atomic unit of work split into
multiple equal-duration sessions because doing it in one sitting is
unrealistic - e.g. "draft the whitepaper" as five 30-minute blocks
rather than one 2.5-hour block. Chunks are *not* sub-tasks. They are
the same task, time-boxed across sessions.

<a id="chunks-vs-projects--the-distinction-matters"></a>

### Chunks vs projects - the distinction matters

- **Project conversion** (`/todo turn-into-project`) decomposes work
  into *different* atomic sub-tasks (research → outline → draft →
  edit → publish). Each sub-task has its own scope and name.
- **Chunking** repeats the *same* atomic work across N sessions. All
  chunks share a name (modulo the `(i/N)` suffix) and a duration.

If the work needs decomposition, use a project. If the work just
needs to be split over multiple sittings, chunk it. When in doubt,
chunking is lighter weight - easier to undo, no project folder,
no `.METADATA.json` plumbing.

### Creating chunked tasks

User phrasings to recognize:

- "split this into 5 chunks of 30 minutes"
- "chunk into 4 sessions of 45 min"
- "break this into 3 thirty-minute blocks"
- "create '<task>' as five 30-min chunks due Friday"

Route these to `brain tasks add` with the
`--chunks N` and `--duration M` flags. The binary handles the
naming, ID issuance, sequential `blocked_by`, and inheritance.

### Chunked-task invariants (enforced by `brain tasks add --chunks`)

- **Naming:** `<base> (i/N)` for i in 1..N. The `(i/N)` parenthesized
  fraction at the end of the name is the canonical chunk marker -
  brain parses it natively; nothing needs to re-derive it.
  Never use a different format (e.g. `<base> [i of N]`); the
  detection logic only recognizes `(i/N)` at end-of-name.
- **Same `due_date`** across all chunks. The deadline is for the
  whole work, not per-chunk.
- **Same `priority`, `task_type`, `project`, `energy_level`,
  `context`, `see_also`, `notes`** - they describe the same work.
- **Each chunk gets its own `estimated_duration`** = the per-chunk
  minutes from `--duration`.
- **Sequential `blocked_by`:** chunk i+1 is `blocked_by` = chunk i's
  `task_id`. Chunk 1 inherits any user-supplied `--blocked-by`.
- **`hard_deadline=true` propagates to all chunks** from chunk 1.
  Rationale: the deadline applies to the whole work; every chunk
  needs to land by then.
- **`mit` goes on chunk 1 only.** Putting it on all five would
  inflate the MIT callout with the same logical task. When chunk i
  is marked done via `brain tasks complete`, the MIT tag automatically
  migrates to chunk i+1 inside the binary. The user always has
  exactly one actionable MIT for the chunked work.

### Surfacing chunks in the agenda / `/todo today` / `/todo what`

For task lists and recommendations, keep prerequisite order and recommend the
lowest unfinished actionable chunk. For a dated family, calculate the minimum
`ceil(remaining chunks / inclusive days to deadline)` (all remaining chunks when
due today/past due); explain that load without placing time blocks or creating
artifacts. Future chunks remain in the store. A request for a scheduled plan
routes to [agenda chunk planning](../agenda/references/planning.md#chunk-planning).

### Defer cascade

`brain tasks defer` is chunk-aware. When the deferred row is part of a
chunk family, later chunks whose `due_date` would otherwise become
earlier than the deferred chunk's new date are **automatically
pushed forward** to preserve the family order. This is handled by
`brain tasks defer`, which cascades the family itself.

Rules:

1. **Cascade only fires on overlap.** If later chunks already have
   `due_date >= the new date`, they are left alone. Example:
   chunks are 1/3 @ 07-01, 2/3 @ 07-10, 3/3 @ 07-15; defer 1/3 to
   07-05 → no cascade (both later chunks are still after 07-05).
2. **Partial cascade.** Each later chunk is compared against the
   running "floor" (the max of the deferred date and any later
   chunk's own due_date). The cascade stops pushing as soon as a
   sibling is already past the floor, and uses that sibling's
   date as the floor for chunks beyond it. Example: chunks are
   1/3 @ 07-01, 2/3 @ 07-05, 3/3 @ 07-20; defer 1/3 to 07-10 →
   2/3 is pushed to 07-10 (overlap), 3/3 stays at 07-20 (already
   later).
3. **`defer_count` is bumped ONLY on the explicitly deferred
   chunk.** Cascaded chunks have their `due_date` and
   `last_touched` updated but **not** their `defer_count`,
   `priority`, or `task_type`. The defer-demote rule (strip mit,
   p0 → p1) applies only to the deferred chunk, not to cascaded
   siblings. The chunk you actually pushed back is the one whose
   slip we want to track; the others are following along to keep
   the order valid.
4. **Earlier chunks are never touched.** Deferring chunk 3/5 only
   considers chunks 4/5 and 5/5; chunks 1/5 and 2/5 are
   irrelevant to the cascade.

### Other edge cases

- **Project linkage:** chunks may belong to a project (set
  `--project <slug>` on `/todo add ... --chunks N`). Every chunk
  inherits the project link, and all chunk IDs land in the
  project's `.METADATA.json:tasks[]` after sync.
- **Renaming:** if the user renames a chunk and breaks the `(i/N)`
  suffix, MIT migration, defer cascade, and family detection stop
  working for that family. Don't rename chunks; if a restructure is
  needed, delete and re-add.
- **Single-chunk "chunks":** `--chunks 1` is rejected - that's just
  a regular task. Use plain `/todo add`.

## Tasks that arrive from the `capture/` in-basket

`<brain>/capture/` is the user's in-basket: the directory they dump notes,
photos, screenshots, PDFs, and scraps of markdown into without deciding what
any of it is. Some of what lands there is reference material and becomes a
note; some of it is an **action** and belongs here instead. Filing an action
as a note buries it, so the classification matters.

`/process-capture` owns the in-basket and its processing pass; see
[its `capture/` section](../process-capture/SKILL.md)
and [the weekly capture pass](../triage/SKILL.md#1b--the-capture-in-basket).
This section is only the task half of that handoff.

- **What arrives as a task.** Imperative phrasing ("email the landlord",
  "book the flights"), a named action, a deadline, a TODO bullet, "remember
  to…", "ask X about…". Also photographic: a snapshot of a receipt to file,
  a screenshot of an error to chase. Classify on what the item is *about*,
  never on its file type.
- **Create it with the normal `brain tasks add` path**, with best-guess
  `priority`, `due_date`, `task_type`, and `estimated_duration`. The
  captured item is usually terse; write a task name that will still make
  sense in three weeks, and put the original wording in `notes` when the
  paraphrase loses something.
- **Keep the material with the task.** When the same captured item also
  produced a note, file the note through `/organize` and put its path in
  the task's `see_also`, so the task carries its context instead of pointing
  at a directory the item has already left.
- **Multi-step scope is a project, not a task.** A captured item with scope
  verbs (`launch`, `build`, `migrate`, `research`) or several checkboxes goes
  through `/todo turn-into-project`: the golden rule against sub-tasks in
  `tasks.csv` applies exactly as it does to any other source. See
  [task-project-link.md](../projects/references/task-project-link.md).
- **The in-basket is not a holding pen.** Once the task exists, the captured
  item leaves `capture/`; it does not stay there as a reminder. The task *is*
  the reminder. An item the user can't decide about becomes a task naming the
  decision, per
  [/triage's "Deferring ≠ leaving in the inbox"](../triage/SKILL.md#deferring--leaving-in-the-inbox).

## Backlog

The **backlog** is for tasks the user wants to keep but isn't going to
act on for the foreseeable future - not abandoned (`/todo remove` is for
that), just parked. It's the pressure-release valve for the chronic-defer
problem: instead of a task slipping week after week and nagging every
triage, it goes to the backlog and goes quiet.

**Semantics (enforced by `brain backlog park`):**
a `status = backlog` task has its `due_date` and `start_date` **cleared**
(a parked task has no schedule; `hard_deadline` and `waiting_since` are
cleared too), is stamped with `backlogged_date = today`, and is **hidden
from everything active** - `is_visible_today` is false, so it never shows
in `/todo what`/`today`/`list`, never lands on an agenda, and is never
surfaced by the at-risk or chronic-ignore scans. It resurfaces only in the
**monthly** triage's backlog-review.

**Entering the backlog:**

- `/todo backlog <task>` directly, or the **"Move to backlog"** action
  during `/triage` (past-due, at-risk, and chronic-ignore walks all offer
  it).
- **Suggest it proactively when `defer_count >= 4`.** A task deferred four
  or more times is one we keep avoiding - offer "move to backlog instead?"
  rather than deferring a fifth time. (This is a personal-assistant
  trigger; see that section.)

**Restoring:** `/todo restore <task>` (or `brain backlog restore <task>`)
flips it back to `not_started` and clears `backlogged_date`;
set a fresh `due_date`/`priority` afterward. The monthly backlog-review is
the main path back (see `/triage`).

**Implicit revive = silent dedupe.** If you re-create a task by hand after
parking the original (an active task whose `created_date` is later than the
backlog task's `backlogged_date`, same name), the monthly triage's
`brain backlog dedupe` treats your manual
re-creation as the intended revive and **silently deletes the now-duplicate
backlog row** - no prompt, no report.

**Auto-purge at 6 months (silent).** A backlog task whose `backlogged_date`
is **more than 6 months ago** (≥6 months + 1 day) is **deleted outright**
by `brain backlog purge`, which
`/triage` runs every pass. This is deliberately silent: **never warn the
user a backlog item is nearing deletion, and never tell them which items
were deleted.** Six months parked = forgotten = fine to forget forever.
The one bookkeeping exception: if a deleted task belonged to a project
(active or archived), the purge leaves a breadcrumb in that project's
`.METADATA.json` (`deleted_backlog_tasks[]`) and `notes.md` so a future
un-archive knows tasks used to exist.

**Backlog ↔ projects.** When you backlog a task that belongs to a project,
you MUST ask the follow-up (via the available question surface (or concise text)):

- **Backlog a project-linked task** → ask: "backlog just this task, or the
  whole project?"
- **Backlogging would empty the project** (it was the last active task, or
  the user chose "whole project") → ask: "archive the project too?" If yes,
  follow [`/projects archive`](../projects/SKILL.md#archive), including its
  IP/completion and linked-task gates, and backlog the agreed tasks through
  native commands.
- **"Backlog this project"** (user names a project, not a task) → interpret
  as: **archive the project + move all its tasks to the backlog.** Confirm
  once, then do both.

Keep the brain project and tasks.csv consistent throughout - same rule as
project conversion (see [task-project-link.md](../projects/references/task-project-link.md)).

**Backlog ↔ external tracker.** The "Backlog ⇒ no dates" invariant applies
to a linked tracker too: parking a task should leave no live deadline on the
tracker side either. The specifics (how to clear a linked issue's date, and
how `/triage` reconciles a tracker's own "Backlog" status) are part of the
tracker workflow - see the `todo:linear` extension point below.

<!-- brain:ext todo:linear-backlog -->

## External issue tracker (optional)

Tasks can carry an optional link to an external issue tracker in the
`linear_issue` column, managed by the local-only commands
`brain tasks set` and
`brain tasks linked`. Core treats this as
inert metadata: an empty `linear_issue` is always fine, and nothing here talks
to any external service.

The **workflow** for syncing tasks with a tracker - filing a task as an issue,
keeping open/closed + `due_date`/`priority`/`title` in sync both ways, mirroring
projects, and the reverse reconcile - is personal (it depends on which tracker
you use and your team's conventions). A `todo:linear` extension supplies it,
typically by pointing at a dedicated tracker-sync plugin.

<!-- brain:ext todo:linear -->

## Personal-assistant triggers

Offer the user help proactively when:

- A task with `defer_count >= 3` shows up → ask if they want to drop
  it or convert to a project.
- **A task with `defer_count >= 4` shows up → offer to move it to the
  backlog** (see [Backlog](#backlog)) instead of deferring it yet again.
  Four+ defers means it keeps getting avoided; parking it stops the
  per-triage nagging without losing it. ("Move to backlog" is also a
  standing action in every `/triage` per-task walk.)
- **A `code` task has no external-issue link** → if a tracker workflow
  is configured (the `todo:linear` extension), it may offer to file the
  task in the tracker. Core makes no such offer on its own; an empty
  `linear_issue` is never treated as drift here.
- A task with `estimated_duration > 90` or scope-verb name
  (`launch`, `build`, `migrate`, `ship`…) → suggest
  `/todo turn-into-project`.
- `/todo today` returns >5 tasks → suggest `/agenda` to
  order them.
- Triage time (10+ past-due tasks) → suggest `/todo triage`.
- **A chronically-ignored task surfaces inline** - i.e. a row whose
  `today - last_touched >= 21d` (or `>= 14d` while `status = in_progress`)
  appears as a candidate in `/todo what`, `/todo today`, `/todo list`,
  project status, or any other surfacing flow. Flag it explicitly:
  > "T### '<name>' hasn't been touched in 28 days. Drop, revive,
  > start now, or convert to a project?"
  Don't silently recommend it as the next action - a stale top
  candidate is itself a signal worth pausing on.
- **The user mentions something they "dropped in capture"**, or you notice
  unprocessed material in `<brain>/capture/` while working → offer to
  process the in-basket (`/process-capture` ["Process capture"](../process-capture/SKILL.md#process)),
  and create tasks for whatever in it turns out to be an action. Don't file
  an action as a note to get the in-basket empty.
- **5+ chronic-ignore hits at once** (run
  `brain tasks chronic --count`
  to check) → suggest `/triage daily`; the chronic-ignore sweep
  (Step 7) is the right pass for clearing a backlog of deadwood.

## When in doubt

- Read [commands.md](references/commands.md) for the full command map.
- Read [task-project-link.md](../projects/references/task-project-link.md) for
  anything touching projects.
- Run `brain tasks lint` (dry-run) before mutating to see what
  state the system is in.
- Ask the user. This is their task system.
