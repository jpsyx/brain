---
name: todo
description: Use when adding, completing, deferring, assigning, or planning tasks in the selected Brain workspace; when asking "what should I work on", "structure my day", or "anything slipping?"; for past-due triage; or for converting an oversized task into a project.
---

# todo

The user's canonical task system lives at `<brain>/tasks/`:

- `tasks.csv` — non-habit tasks.
- `habits.csv` — recurring habits. The only recurring rows allowed.
- `SCHEMA.json` — machine-readable schema.

Throughout, `<brain>` is the selected workspace root from `BRAIN_ROOT`;
`~/.agents/skills/todo/scripts/` is where `brain skills sync` installs this
skill's helper scripts; `$AGENDA_DIR` is `brain config get
agenda_dir` (the folder the agenda PDF is written to, default your Downloads
folder); and `markdown-to-pdf` is the configured PDF command
(`markdown_to_pdf_path`) — run that path, not a same-named shell
alias.

**You are a personal assistant first, command executor second.**
Answer "what should I work on?" / "structure my day" / "anything
slipping?" as fluently as you execute `add` / `done` / `defer`.

Load who you're assisting: run `brain personalize show` and honor their
`role`/`works_for` (both may be unset — then stay neutral). **Top priority:
save the user's time.** Be blunt. Make obvious decisions yourself; do not ask
the user trivial questions. When the day cannot fit everything the user wants
to do, surface that tradeoff explicitly and propose what to drop or push —
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
  rows (including done and/or habits), they would say so — e.g.
  "all tasks including done", "tasks and habits", "show habits
  too", "completed tasks this week". Apply the same default in
  counts, lists, summaries, and natural-language answers. When a
  query is genuinely ambiguous (e.g. the user just spoke about
  habits and then asks "how many?"), prefer the default scope but
  briefly note that habits/done aren't counted so the user can
  ask for the broader scope if they meant it.
- **Golden rule: no sub-tasks in tasks.csv.** A task that wants
  sub-tasks is a project — offer `/todo turn-into-project`. See
  [task-project-link.md](references/task-project-link.md).
- **Backlog ⇒ no dates, ever (hard invariant — backlog and dates are
  mutually exclusive).** A `status=backlog` item carries **no schedule**:
  empty `due_date`, empty `start_date`, `hard_deadline=false`, empty
  `waiting_since`. Being backlogged and having any date are mutually
  exclusive states — if it has a date it isn't backlogged; if it's
  backlogged it has no date. A date on a backlog row is never "maybe it's
  mis-statused"; it is always drift to be corrected **by clearing the
  date**. `backlog_task.py` enforces this on entry (clears all four);
  never hand-set a `due_date` on a backlog row afterward. If a backlogged
  task is linked to an external issue tracker, keeping that side's date in
  sync is the tracker workflow's job (see the `todo:linear` extension point).
  See [Backlog](#backlog).
- **`blocked` vs `waiting` — and no-penalty defers.** Two different
  reasons a task can't move, tracked two different ways:
  - **Blocked** = waiting on *another task we own*. Recorded in the
    `blocked_by` column (pipe-separated `T###`), not a status.
  - **`waiting`** = a status value, for tasks paused on **external**
    circumstances/people we don't control (a reply, a vendor, a legal
    review). When you set `status=waiting`, also stamp `waiting_since`
    with today's date.

  In **both** cases the slip isn't avoidance, so **deferring does NOT
  increment `defer_count`** and the defer-demote rule is skipped (no
  stripping mit, no p0→p1). `defer_task.py` applies this automatically
  when `status==waiting` or `blocked_by` is non-empty; `--no-count`
  forces it for any other genuinely-not-our-fault push. `defer_count`
  stays the "are *we* ignoring this?" signal — it should only climb on
  our own avoidance.
- **Waiting can't be forever.** A task stuck in `waiting` for more
  than 7 days (by `waiting_since`) should trigger a nudge: the
  assistant offers to follow up with the external party (infer who
  from the task name if possible) and to create a check-in task.
  Detector: `python3 scripts/find_stale_waiting.py` (`--count`,
  `--pretty`, `--threshold N`). Run it in the agenda flow and in
  `/triage`.
- **Bidirectional task ↔ project link.** Always validated by
  structured CLI tools (Python diff), never LLM judgment.
- **Optional external-issue link.** A task may carry a link to an
  external issue tracker in the `linear_issue` column, managed by the
  local scripts [`set_linear_issue.py`](scripts/set_linear_issue.py) /
  [`list_linked_tasks.py`](scripts/list_linked_tasks.py). Core treats it as
  inert metadata — an empty value is always fine and nothing here contacts an
  external service. The sync *workflow* (when/how to mirror to a tracker) is
  personal; see the [External issue tracker](#external-issue-tracker-optional)
  section and its `todo:linear` extension point.
- **Assignment follows the effective actor.** Every new task and habit defaults
  `assigned_to` to the immutable effective actor in `BRAIN_ACTOR_ID`, whether
  the workspace has one member or several. Unrelated edits never change it.
  Explicit assignment uses `--assigned-to <user-id>` and explicit reassignment
  uses `reassign_task.py <task> <user-id>`; both validate the ID through the
  selected workspace's portable `.config/users.json`. One-person workspaces
  hide assignment detail, creation/reassignment controls, and filters while
  still filling the ID. Shared workspaces show those surfaces and accept
  `assigned_to=<user-id>` as a list filter.
- **Short task IDs are the user-facing handle.** Tasks use `T###`
  (e.g. `T17`), habits use `H###` (e.g. `H42`). Issued by
  [`scripts/next_id.py`](scripts/next_id.py); counters live beside the selected
  workspace's CSVs. Scripts require Brain's workspace environment and never
  fall back to a home-directory brain.
  `task_uuid` is the immutable merge identity: new rows and spawned habit
  occurrences receive UUIDv4, while edits and completion preserve it.
  `task_id` remains the mutable display identity used by commands. Do not edit
  display IDs by hand. **Name-fragment matching still works for
  input** — IDs are shorthand, not a replacement. See
  [commands.md](references/commands.md) for the `<task>` resolution rules.

## Operating principles

1. **Never edit CSVs by hand.** Use [scripts/](scripts/) — they keep
   `defer_count`, `completed_date`, `last_touched`, habit-spawn, and
   link consistency correct. `last_touched` is auto-bumped by every
   mutator (`add_task.py`, `defer_task.py`, `defer_habit.py`,
   `brain habits skip`, `brain tasks complete`, `touch_task.py`,
   `backlog_task.py`, `set_linear_issue.py`) so chronic-ignore
   detection and CSV sync have real recency signals to work with; if
   you ever read-modify-write a row outside those
   scripts, call `_csvlib.touch_row(row)` before writing.
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
5. **Today is what the system clock says — NOT what your context
   says.** Before any date-sensitive action, re-derive "today" by
   running `date +%F` (or `date.today()` in Python) in the shell and
   use *that* value. **Do NOT trust the `currentDate` / "Today's date
   is …" value from your session's opening context** — that is frozen
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
   shell wins — every time.
6. **Always pair the `task_id` with the task name — never reference
   a task by ID alone.** Anywhere a task appears in user-facing
   output — `/todo today`, `/todo agenda`, `/todo what`,
   `/todo list`, triage groups, search results, suggested orders,
   cut orders, parenthetical mentions, even casual inline prose —
   show **both** the `T###` / `H###` AND the task name (or a clear
   shorthand when space is tight, e.g. "T75 download policy").
   Neither half alone is sufficient:
   - **IDs alone** (e.g. "Drop T49 → T47 → T61") are unreadable on
     a printed agenda or anywhere the reader can't scroll back for
     context — and agendas WILL get printed.
   - **Names alone** force the user to re-type titles to act on
     tasks (`done T17` vs `done <full title>`).
   In tables, give IDs their own leftmost column. In ordered prose
   lists (suggested order, cut order, ad-hoc sequences), the format
   is `**T###** short name` or `**T###** (short name)`. A shorthand
   is fine — full titles aren't required — but the shorthand must
   be unambiguous to a reader who hasn't memorized the ID map.
7. **Persist agendas to disk + generate a printable PDF.**
   Whenever you build an agenda (`/todo agenda`, `/todo plan-day`,
   or any time the user asks for "the agenda for X"), write the
   rendered markdown to `/tmp/<TARGET_DATE>.md` where
   `<TARGET_DATE>` is the agenda's date in `YYYY-MM-DD`. Overwrite
   if the file exists. When the user reworks an existing agenda —
   or completes/defers/drops tasks that appear on it — update the
   same file so it stays current. The user opens these files via
   the `agenda` zsh function (`agenda today`, `agenda tomorrow`,
   `agenda 2026-06-09`, or bare `agenda` for the latest).

   **Task-mutation auto-update — handled by the mutator paths.**
   `defer_task.py`, `defer_habit.py`, and `touch_task.py` each invoke
   [scripts/update_agenda_on_mutation.py](scripts/update_agenda_on_mutation.py)
   at the end of a successful mutation. That script performs the
   full checklist programmatically: drops the task from MIT
   callout / Suggested order / Cut order (with chunked-task
   next-sibling swap on `done`), re-derives Today's habits and
   Completed today from the CSVs, and regens
   `$AGENDA_DIR/agenda-<today>.pdf` only if a PDF already exists
   on disk (no PDF → no regen, per the carve-out below). Idempotent
   — safe to re-run, no-op when there's nothing to do.

   Completion is native in the `brain` binary: use
   `brain tasks complete <id>` for tasks and habits. If the completed
   item appears on an already-written agenda, update that agenda as
   part of the same workflow.

   **You do NOT need to run any of that yourself after invoking a
   mutator script.** Don't grep the agenda, don't rewrite the
   markdown, don't regen the PDF — the scripts already did it.
   Trust the side effect and move on. The only time you should
   touch `/tmp/<today>.md` directly is when the user explicitly
   asks for an agenda change that isn't a mutation (e.g. "redo the
   Suggested order", "swap T48 and T54", "rebuild from scratch"),
   when you complete a task/habit via `brain tasks complete`, or
   when you read-modify-write a CSV row by hand instead of via
   a script (which you shouldn't — see operating principle 1).

   The script's behavior, for reference (you don't need to
   reproduce it):

   1. Stop if `/tmp/<today>.md` doesn't exist.
   2. Surgical edits to MIT callout / Suggested order / Cut order:
      drop lines referencing the mutated `T###`/`H###`, renumber
      Suggested + Cut. On `done` for a chunk with an unfinished
      next sibling, the next chunk swaps into the just-vacated
      MIT-callout and Suggested-order slot (so the user always
      has exactly one actionable chunk visible).
   3. Re-derive Today's habits and Completed today from the CSVs
      every run — catches habits flipped to done outside this
      session (other Claude runs, /triage, manual edits).
   4. Regen `$AGENDA_DIR/agenda-<today>.pdf` only if it already
      exists. No PDF on disk → skip (a CSV mutation isn't a
      request for a fresh printout; if the user wants one they'll
      ask). This is the one carve-out to the "After every agenda
      write…" rule below.

   **After every agenda write (initial OR update), also generate
   a printable PDF in `$AGENDA_DIR/` — except for the
   task-mutation carve-out above when no prior PDF exists.** The
   user prints agendas to carry through their day, so once a PDF
   exists for a date it must stay current. **Do not open the PDF
   automatically** — only open it when the user explicitly asks
   (e.g. "open the agenda", "show me the PDF", "let me see it").
   Run, in order:

       rm -f $AGENDA_DIR/agenda-<TARGET_DATE>.pdf
       markdown-to-pdf /tmp/<TARGET_DATE>.md --out $AGENDA_DIR/agenda-<TARGET_DATE>.pdf --agenda

   Then **verify the page count**:

       python3 -c "from pypdf import PdfReader; print(len(PdfReader('/agenda-<TARGET_DATE>.pdf').pages))"

   Target: 2 pages. A 3rd page is acceptable **only** if the
   overflow is the closing "Completed today" table — that
   section grows as the day progresses and is reference-only,
   so letting it spill is fine. If the agenda body (MITs,
   Suggested order, Cut order, Today's habits) itself runs
   past page 2, re-run the conversion with `--font-shrink 1`,
   then `--font-shrink 2`, and so on (1 pt at a time) until
   the body fits on 2 pages. Always tear down the previous
   PDF with `rm -f` between attempts so the versioned-collision
   fallback doesn't kick in.

<!-- brain:ext todo:agenda-after-build -->

   When the user does ask to open it, run:

       /usr/bin/open $AGENDA_DIR/agenda-<TARGET_DATE>.pdf

   Five implementation notes, all load-bearing — do not
   substitute aliases or drop the flags:

   - **The `rm -f` is required, not optional.** The PDF script's
     default in non-interactive contexts is to write a versioned
     file (e.g. `agenda-…-v2.pdf`) on collision — never to
     clobber. For agendas we want the opposite: every update
     silently replaces the existing PDF, so the user has exactly
     one always-current `agenda-<YYYY-MM-DD>.pdf` in Downloads
     (no stale versions piling up, no ambiguity about which one
     to print).
   - **`--agenda` is required.** This is a single shortcut flag
     that bundles the no-table-borders + compact-tables styling
     the agenda needs. Without it, the "Today's habits" and
     "Completed today" tables render with the default cell
     grid + header fill + standard padding — which looks like a
     spreadsheet and reliably bleeds the printout onto a 3rd
     page. The bundled `--agenda` form is intentional so a
     caller (this skill, `/triage`, or any future flow that
     regens the agenda PDF) can't accidentally pass one flag and
     forget the other. The default-with-borders behavior is the
     right call for every OTHER markdown→PDF use case, so it
     stays opt-in via this flag rather than flipping the global
     default. `--no-table-borders` and `--compact-tables` still
     work individually for non-agenda docs that want one but
     not the other.
   - **`--font-shrink` is the escalation lever, not the
     default.** Only add it when the first pass exceeds 2 pages.
     `--font-shrink 1` reduces every text style's fontSize and
     leading by 1 pt globally (still per-document — the default
     stays at the standard sizes for every other PDF flow).
     Increment by 1 pt at a time so the shrink is no harsher
     than necessary. For a typical extended-schedule agenda you
     should converge at `--font-shrink 1` or `--font-shrink 2`.
     If you find yourself past `--font-shrink 3` and still
     spilling, **stop shrinking and trim instead** per the
     "2-page hard cap" priority list — abbreviating habit names
     and Suggested-order names buys page space without making
     the printout uncomfortable to read.
   - **Use `/usr/bin/open`, not bare `open`** when the user does
     ask to open the PDF. The user has an `open` autoload
     function that fails to resolve in non-interactive shells;
     the system binary at `/usr/bin/open` is always available
     on macOS and behaves identically.

   Filename is fixed: `agenda-<YYYY-MM-DD>.pdf` with the
   `agenda-` prefix, to disambiguate from other files in
   Downloads.
8. **Agendas are presentation-grade — write them as if you'll print
   the file and hand it to a manager who will run with it for the
   day.** This applies on both first build and every subsequent
   update. The manager test: can they read it top-down and know
   exactly what to do, in what order, and which items absolutely
   cannot be missed — without asking a follow-up question?
   - **Markdown blank-line rule.** Markdown collapses adjacent
     non-blank lines into a single paragraph; only a *blank line*
     produces a paragraph break in the rendered PDF. So whenever
     you want two visually separated paragraphs — e.g. the
     `**Load:** …` and `**Bottom line:** …` one-liners at the top,
     or any two short labelled lines that should sit on their own —
     put a **blank line between them**. Without it the PDF runs
     them together as one wrapped block, which is the single most
     common agenda-rendering bug. Same rule for the gap between a
     paragraph and the list that follows it, and between sections.
     Do not rely on a trailing two-space "hard break"; use blank
     lines for any separation a reader is meant to see.
   - **Load and Bottom line are *one-liners*. Keep them ruthlessly
     short.** They sit at the top of page 1 as a glance-summary, not
     a recap. If a reader has to read more than a sentence to get
     the gist, the line is failing.

     - **Load** is just a load descriptor word (`light`, `easy`,
       `normal`, `heavy`, `overcommitted`), optionally followed by
       a single short sentence explaining *why* in plain language.
       No task IDs. No counting hard-deadline hours. No
       enumerating what got triaged. Examples:

       - `**Load:** Heavy. Hard-deadline State Dept materials are the spine.`
       - `**Load:** Light. One MIT, otherwise admin and habits.`
       - `**Load:** Overcommitted. Too many hard deadlines collide.`

       Anti-pattern: `**Load:** Still heavy. Triage done. T38/T54
       deferred to 2026-07-02, T2/T5 dropped, T15/T19/T21/T23/T30
       revived. T13/T43/T47/T79 now in_progress…` — that's a
       changelog, not a load descriptor.
     - **Bottom line** is **one statement, max two.** Two only
       when there are genuinely two unrelated must-knows for the
       day (e.g. one strategic anchor + one risk flag). Anything
       more is noise.

       Every task ID in the Bottom line is paired with the task's
       short name in parentheses: `T97 (whitepaper)`, `T61 (June
       commitments)`. **Never bare IDs here** — the reader is
       glancing at the top of a printed page; they cannot scroll
       back to map IDs to names. The same goes for any other
       narrative line in the agenda body (the rest of the agenda
       uses `**T###** name` per operating principle 6, but the
       Bottom line uses `T### (name)` because it reads as prose).

       Examples:

       - `**Bottom line:** Protect AM for T97 (whitepaper, 2h).`
       - `**Bottom line:** T61 (June commitments) is 6d past its
         hard deadline — if it slips again today, reset rather
         than carry.`
       - `**Bottom line:** Protect AM for T97 (whitepaper). T61
         (June commitments) is 6d past hard deadline — reset if
         it slips again.`  *(two statements, both load-bearing)*

       Anti-pattern: a Bottom line that restates the MIT list,
       summarizes triage outcomes, or chains three+ sentences
       of context. That belongs in conversation, not the
       printout.
   - **Allowed sections — exactly five, in this order.** An
     agenda is composed of these `##` sections and nothing else.
     Do NOT add critical-path detail, hard-deadline detail,
     today-pulled lists, habit catch-up sections, quick wins,
     pre-empt-tomorrow, past-due tail, triage outcomes, or any
     other narrative section — they're noise on a printed page
     and dilute the signal. Anything load-bearing must fit
     inside one of these five:

     1. `## ❗ MITs — if only these get done, today is a win`
     2. `## Suggested order`
     3. `## Cut order if the day slips`
     4. `## 🔁 Today's habits`
     5. `## ✅ Completed today` (omitted when nothing's done yet)

     The title block (`# YYYY-MM-DD — <weekday> agenda`), the
     `**Load:** …` one-liner, and the `**Bottom line:** …`
     one-liner sit above the first `##` and are not "sections"
     in this count — they're the page header.
   - **Page-1 contract.** The first page of the printed PDF
     holds, in this exact order: (1) title + load + bottom
     line, (2) the MIT callout, (3) the "Suggested order"
     block, (4) the "Cut order if the day slips" block. These
     answer the two questions the manager needs first: *what
     are the non-negotiables?* and *what do I actually do, in
     what order?* The remaining two sections (Today's habits,
     Completed today) live on page 2. **Do not put Suggested
     order or Cut order at the bottom of the file** — that
     buries the literal agenda. If the MIT callout balloons past
     ~12 items, abbreviate names to keep the suggested/cut
     blocks on the same page.
   - **Top priorities are unmissable.** Use emoji prefixes on
     section headers and on individual items that absolutely
     cannot slip. Emoji vocabulary (do not improvise others):
     **❗ = MIT** (matches the `mit` task_type display label in
     SCHEMA.json — any task with `mit` in `task_type` gets ❗,
     full stop); **🔥 = critical path** (the can't-fail sequence
     for the day, often but not always overlapping with MITs);
     **⚠️ = hard deadline** (`hard_deadline=true`, never deferred
     without explicit user confirmation); **✅ = completed today**
     (only in the closing "Completed today" section — never on
     items still in flight). Don't sprinkle emojis decoratively
     — reserve them for "if you read nothing else, read these."
   - **MIT callout — page-1 section 2.** Immediately after the
     title/load line. Lists every today-eligible MIT as a flat
     **checkbox one-liner** (`- [ ] ❗ **T###** name (duration)`),
     in execution order. Headline framing: "If only these get done,
     today is a win." Use markdown task-list checkboxes (`- [ ]`),
     not plain bullets — the user prints the agenda and physically
     ticks items off. This callout is the manager's at-a-glance
     view of non-negotiables. MITs also appear in the Suggested
     order (where they sit in actual execution sequence) — the
     callout is the "if you read nothing else, read these"
     summary, the Suggested order is the playbook.
   - **Suggested order + Cut order — page-1 sections 3 and 4.**
     Immediately after the MIT callout. The suggested order is
     the concrete sequence to attempt for the day — habits and
     all, top to bottom — rendered as a **numbered checkbox
     list** (`1. [ ] **T###** name`, `2. [ ] …`), so the user can
     both follow the sequence and tick items off the printed
     page. **Exception: scheduled non-task items** (recurring
     anchors that aren't tracked as a task or habit, e.g. a standup, a
     workout, lunch, and **calendar
     busy blocks** pulled from `<calendar_id>` —
     meetings, therapy, focus time, calendar "Busy"
     holds) appear in the list in chronological position but
     **without a number and without a checkbox** — they are
     fixed events, not items the user picks up and completes,
     so they shouldn't consume a numbered slot or invite a tick
     mark. The numbered sequence skips over them and continues
     across the actionable items only. The cut order is what to drop first when reality
     compresses, in the order to drop them — rendered as a
     **plain numbered list (no checkboxes)**, since cuts aren't
     things you complete. **Cut order has a hard cap of 5
     items.** More than 5 dilutes the signal — the cut order is
     the manager's "if I'm running behind, drop these in this
     order" cheat sheet, not a long fallback list. Pick the 5
     most-droppable items from the Suggested order, in the order
     you'd drop them. Never include MITs or hard-deadline-today
     items in cut order — those are non-negotiable, by
     definition. These two blocks together ARE the agenda; the
     rest of the file just expands on individual items. They
     live on page 1 next to the MIT callout so a manager
     glancing at the printout has both "what matters" and "what
     to do" in the first thing they see.
   - **Checkbox glyphs per section.** Each of the five
     sections has a fixed checkbox convention; don't mix or
     improvise:

     - **MIT callout:** GFM task-list `- [ ] ❗ **T###** name …`
       — the PDF renders these as bullet checkboxes the user
       physically ticks on the printed page.
     - **Suggested order:** GFM task-list `1. [ ] <time> | **T###** name …`
       — numbered, with the checkbox glyph. Scheduled non-task
       items (a standup, a workout, lunch, calendar busy blocks from
       `<calendar_id>`, etc.) are the exception:
       render as a plain line `<time> | <name>` — **no number,
       no checkbox, no list marker** — in their chronological
       position. The numbering on the surrounding actionable
       items keeps flowing past them.
     - **Cut order:** plain numbered list `1. **T###** name …`,
       **no checkboxes** — cuts aren't things you complete.
     - **Today's habits:** inline `◻` / `✅` glyph at the start
       of each table cell (`◻ **H##** name` for pending,
       `✅ **H##** name` for done-today). Not GFM task-list
       syntax — those don't parse inside table cells.
     - **Completed today:** inline `✅` glyph at the start of
       each table cell (`✅ **T##** name`). Every cell is
       ticked because every cell is, by definition, a thing
       that was completed today.

     `✅` is the canonical "this is done" glyph throughout the
     agenda — don't substitute `☑` (ballot box with check) or
     other check-style codepoints. One look means one thing.
   - **"Today's habits" reference section — second-to-last.**
     Just before the closing "Completed today" log, render a
     `## 🔁 Today's habits` section listing **every** habit the
     `habits` command would show — i.e. every row in habits.csv
     where `status != done` AND (`due_date` is empty OR
     `due_date <= today`), PLUS every habit marked done today
     (`status == done` AND `completed_date == today`). This is
     a glance-reference snapshot of habit state for the day:
     pending habits get an empty checkbox, already-done habits
     get a ticked checkbox. The user still ticks from the
     Suggested order during execution; this section is the
     printed snapshot.

     - **2-column markdown table, no header text.** The first
       row is the GFM-required separator only; cells in body
       rows hold the habits. Each cell starts with a checkbox
       glyph — `◻` (U+25FB) for pending, `✅` for already-done
       (same glyph "Completed today" uses, so "done" looks
       identical everywhere on the page). Render the table
       directly under the heading — no explanatory paragraph
       above it. Example:

       ```markdown
       ## 🔁 Today's habits

       |  |  |
       |---|---|
       | ✅ **H10** Meds | ◻ **H11** Stretch |
       | ◻ **H12** Morning Reading | ◻ **H13** Mid-day Reading |
       | ◻ **H14** Afternoon Reading | ◻ **H15** Morning walk |
       | ◻ **H16** Evening walk | ◻ **H18** Social media harvest |
       ```

     - **`<glyph> **H###** name`** per cell — the same ID + name
       pairing rule (operating principle 6). Abbreviate names
       only if a row would wrap; the goal is one habit per cell,
       two per row, no wrapped lines.
     - **Fill order: pending habits first**, sorted by
       `ideal_time` ascending (chronological — earliest first).
       Habits with an empty `ideal_time` sort last (treat as
       "Anytime"). Ties (same `ideal_time`, including empty)
       break by `estimated_duration` asc → name. **Then
       completed-today habits** at the end, sorted the same way.
       Left-to-right, top-to-bottom within those two groups.
       Note: the agenda lists habits in pure chronological order —
       it does NOT sub-group by Morning / Afternoon / Evening
       (that grouping lives on the `habits` HTML page only).
     - **Habit `start_date`** still applies: if a habit's
       `start_date > today` (rare for habits, but possible if
       deferred via `/todo defer-habit`), exclude it.
     - **Section is omitted when there are 0 habits to show**
       (no pending + no completed-today), same rule as
       "Completed today" — never render an empty reference
       table.
   - **2-page body cap (Completed today may spill).** The
     agenda body — MIT callout, Suggested order, Cut order,
     Today's habits — must fit on **at most 2 US-Letter pages,
     default margins.** Page 1 holds the four page-1-contract
     sections; page 2 holds Today's habits. The closing
     "Completed today" table is allowed to overflow onto a 3rd
     page — it grows as the day progresses and is
     reference-only, so spilling it is fine. But the body itself
     spilling past page 2 is a fit problem to solve. Trim in
     this priority order — drop the first thing whose loss
     doesn't change what the manager needs to execute:

     1. **Today's habits cell names** — abbreviate aggressively
        (e.g. `**H12** Morning Read` → `**H12** AM Read`,
        `**H22** Afternoon Inbox & Readings` → `**H22** PM Inbox`).
     2. **Suggested-order item names** — abbreviate while
        keeping the ID and ballpark time intact.
     3. **MIT-callout item names** — abbreviate as a last
        resort; the IDs and duration must stay.
     4. **Split the Today's habits table** so the section
        straddles the page break. Never split the MIT callout,
        Suggested order, or Cut order.

     Never cut any of the five allowed sections entirely. They
     are all load-bearing.
   - **"Completed today" — last section, two-column table.**
     At the very end of the file, under a `## ✅ Completed today`
     heading, render a **2-column markdown table, no header
     text** of every task AND habit marked `done` today
     (`status == done` AND `completed_date == today`). Each
     cell starts with `✅` followed by the ID + name (the same
     ID + name pairing rule, operating principle 6) — no `-`
     bullet, no list markers. The ticked glyph in each cell is
     the signal; bullets are redundant. Fill left-to-right,
     top-to-bottom. Example:

     ```markdown
     ## ✅ Completed today

     |  |  |
     |---|---|
     | ✅ **H34** Meds | ✅ **H35** Morning Triage |
     | ✅ **H29** Workout | ✅ **H28** Stretch |
     | ✅ **T49** Speaker exchange |  |
     ```

     **Do not track drops, defers, conversions, replacements,
     or adds in the agenda.** The agenda is a forward-looking
     plan, not an audit log; those decisions are noise on a
     printed page. If nothing has been completed yet, omit the
     section entirely — never render an empty table. (Project
     conversion, task adds, etc. ARE tracked elsewhere — in
     tasks.csv, in .METADATA.json `tasks[]`, in commit history.
     They don't belong in the printable plan.)
   - **Update means rewrite, not patch.** When you update an
     existing agenda, the result should look like it was written
     fresh — no stale labels ("NEW: …", "just added"), no empty
     sections left from removed items, no contradictory time
     estimates from prior passes. Recompute totals every time.
     If a task got dropped/deferred/converted since the last
     render, it simply disappears from the agenda — no note, no
     strikethrough, no closing remark.

     **Specifically: re-derive "Today's habits" and "Completed
     today" from `habits.csv` + `tasks.csv` on every update, not
     from the prior agenda.** It is tempting (and wrong) to
     forward-copy these two sections from the previous render
     because the diff "feels" small. Don't. Habits get marked
     done outside our session window — by the user manually, by
     other Claude sessions, by `/triage` — and the prior agenda
     becomes stale the moment a habit's `completed_date` flips.
     The mandatory re-derive procedure:

     1. Re-read `habits.csv`. For "Today's habits": include every
        habit where (`status != done` AND
        (`due_date == ''` OR `due_date <= today`)) PLUS every
        habit where (`status == done` AND
        `completed_date == today`). Habit with `completed_date
        == today` → `✅` glyph; otherwise → `◻`.
     2. Re-read both CSVs for "Completed today": every row where
        `status == done` AND `completed_date == today`.
     3. Re-scan the Suggested order: any row whose ID has
        `status == done` (in either CSV) is removed and the list
        renumbered.

     Use a quick Python pass (`csv.DictReader`, not
     `awk -F','` — habit notes contain quoted commas and
     newlines that break naive CSV parsing) before writing the
     agenda. The cost is one extra heredoc; the payoff is the
     printed agenda actually matches reality.
   - **Use ballpark times, not exact times, in the Suggested
     order.** The agenda is a *blueprint*, not a strict calendar.
     The user does not want exact times to punish themselves with
     when they fall behind.

     Rules:
     - Granularity: no finer than 30-min intervals.
     - Use `~` (squiggly line / tilde) to mean "approximately":
       e.g. `~11 AM`.
     - Multiple suggested-order items at the same ballpark time
       is FINE — the user knows they can't do two things
       simultaneously. Two tasks both labelled `9:30 AM` just
       means "both happen around 9:30, in this order".
     - Do NOT hyper-pack the agenda with minute-by-minute precision.

     Format the suggested-order line as:
     `<n>. [ ] <time> | **T###** name (duration)` — keep the
     checkbox. **Scheduled non-task items** (a standup, a workout,
     lunch, etc.) render as a plain `<time> | <name>` line in
     chronological position — no `<n>.` prefix, no `[ ]` glyph.
     The numbered actionable items keep counting past them.

     Example:

     ```
     9:00 AM | Team standup (30m)
     1. [ ] 9:00 AM | **H35** Morning Triage (run /triage)
     2. [ ] ~9:30 AM | **T76** Read privacy policy (45m)
     3. [ ] ~10:30 AM | **T77** Draft response to lawyers (30m)
     12:30 PM | Lunch (30m)
     12:15 PM | Lunch (20m)
     ```
9. **Building an agenda is NOT the morning triage habit.**
   Preparing the agenda is plan-the-day. Morning triage is the
   actual process of walking past-due tasks (now handled by
   `/triage`). They are separate. Mark the "Morning Triage"
   habit done in exactly two cases: (a) the user has actually
   completed a triage pass, or (b) the user **explicitly says to
   skip daily triage** for the day (see next paragraph). Never mark
   it done just because you built the agenda.

   **"Skip daily triage today" ⇒ skip the Morning Triage habit
   (explicit user rule).** When the user says we can skip daily
   triage ("skip daily triage", "no triage today", "we can skip
   triage") — including in passing while you're building the agenda
   — run `brain habits skip "Morning Triage"` and run no triage pass.
   Morning Triage is a **daily** habit, so per the general
   [Skipping a habit](#skipping-a-habit) rule that marks today's
   occurrence `done` — there is no daily-triage-specific skip path
   anymore; it's just the daily-habit skip. Skipping is a decision
   that the day is handled, not a decision to leave the habit
   pending. Marking it done is what stops the `tasks` TUI
   (the brain tasks view) from nagging at startup — that modal is
   gated on this habit via `daily_triage_name_pattern: "Morning
   Triage"` → `App::check_daily_triage` — and keeps the agenda's
   habit state honest. Do it immediately; don't ask. (Full
   rationale lives in `/triage` SKILL.md Step 9.)

   When an agenda lists the morning-triage habit in "Today's
   habits" or "Suggested order", do NOT annotate it with
   "covered by building this agenda" or any similar hand-waving.
   Just list it as a habit to do.
10. **When the day can't fit everything, say so — bluntly.** If
    the user's pending tasks literally cannot be done in the
    remaining day given the daily anchors and constraints, that's
    OK — your job is to surface the tradeoff:
    - State plainly that everything won't fit.
    - Propose specifically what to drop or push to tomorrow.
    - Ask the user to confirm a tradeoff. Never silently overstuff
      the agenda and pretend.
    - Always make a recommendation; don't just dump the choice on
      the user.
11. **Flag hard deadlines explicitly when building the agenda.**
    Tasks with `hard_deadline=true` are must-dos for today (or
    for whichever day they're scheduled). For any hard-deadline
    task whose `defer_count >= 1`, pause and ask: "T### was
    deferred [N] times — is the hard deadline still real, or do
    you want a more realistic date?" Get the answer before
    treating the deadline as immovable. This is part of the
    first-agenda-preparation question batch (see "First agenda
    preparation").
12. **Any action-choice prompt uses `AskUserQuestion`, not prose.**
    Any time this skill (or `/triage`) asks the user to pick among
    concrete actions — past-due triage, at-risk preview,
    chronic-ignore sweep, agenda cut decisions, any 1-by-1
    disposition flow, **and any closing or follow-on offer**
    ("convert these to projects?", "slot this into the agenda or
    leave it?", "want me to do A or B?") — use the `AskUserQuestion`
    tool with up to 4 questions per call (one task per question).
    This is **not** limited to the named flows above; the tell is
    simple — **if the answer is the user choosing among options,
    it's an `AskUserQuestion` call**, including the wrap-up at the
    end of an agenda build or triage run. Clickable options are
    faster for the user and let you batch 4 tasks per round instead
    of walking 1-by-1 over text. Pick the 4 most contextually
    relevant options (typical default: `Done` / `Defer +7d` /
    `Drop` / `Start now`); the auto-added "Other" handles
    less-common actions (`defer to date`, `change priority`, `mit`,
    `convert-to-project`, `revive`, `skip`). The full vocabulary
    lives in `/triage`'s "Asking the user for per-task actions"
    section. Use prose **only** for the narrow set that isn't a
    fixed action menu — hard-deadline confirms, open scoping
    tradeoffs, in-basket UNSURE clarifications.

    **Make the question impossible to miss.** The user has said
    they will not read long reasoning in triage/agenda flows. Keep
    any lead-in prose to a line or two, format it to stand out
    (bold / a table / a `---` rule), and put the decision, context,
    and trade-offs **inside the `question` field and the option
    labels/descriptions** — never bury the real choice or the info
    needed to make it in skippable prose above the tool call.
13. **Every agenda includes at least 2 code-work items.** The agenda's
    Suggested order must always schedule **a minimum of 2 items from the
    code-work pool** — coding tasks (`task_type` contains `code`) and
    PR-review tasks (the `Review PR: …` tasks /triage creates, which are
    also `code` type) combined. Any mix counts: 2 coding, 2 PR reviews,
    or 1 of each. **More than 2 is encouraged** when capacity allows —
    2 is the floor, not a cap. PR-review tasks pull slightly ahead of
    equal-priority coding tasks (an unreviewed PR blocks a teammate).
    **The one carve-out:** drop below 2 only when including them would
    make a higher-priority or hard-deadline-today item unachievable —
    i.e. there genuinely isn't room. In that case, say so explicitly
    (operating principle 10) rather than silently omitting code work,
    and schedule as many code-work items as do fit (1, or 0 only if even
    one won't fit). If there are fewer than 2 code-work items available
    in tasks.csv at all, schedule what exists and note it — don't
    invent work. Place these during Phase 5/6 like any other task,
    respecting windows and anchors.

## First agenda preparation (collaborative working mode)

The FIRST time today's agenda is built (no `/tmp/<YYYY-MM-DD>.md`
exists for today yet), treat it as a **collaborative working
draft**, not a finalized plan. The agenda is a *working* document
until the user explicitly says it is finalized / locked-in.

Expect the user to review and tell you to drop / change /
substitute items. That's the design.

**Before generating, you MAY ask up to 2-3 high-leverage
questions** — but only if you genuinely can't proceed without an
answer. Examples of good questions:

- "T48 and T54 are both ~30 min p0 MITs but I only have one open
  slot before lunch — which goes first?"
- "T79 was deferred once. Hard deadline still real, or push?"

Examples of BAD questions (don't ask these):

- "Should I include your daily habits?" (yes, obviously)
- "Do you want the agenda by priority?" (use the documented format)
- Anything that could be decided by reading the task list yourself.

**Always ask about already-completed habits, scoped to current
time.** Check the wall-clock time when you were asked to build the
agenda. If it's past the time some daily habits would normally
have happened by now (e.g. it's 10 AM and "Meds", "Stretch",
"Morning Reading" are still `not_started` in habits.csv), the user
has very likely already done some of them off-system. The CSV
won't reflect that until they tell you.

Before writing the agenda, ask once — batched into a single
question:

> "It's [time]. Have you already done any of today's habits? I'll
> mark them done so they don't clutter the agenda: Meds, Stretch,
> Morning Reading, …" (list only the still-`not_started` habits
> whose typical time window has already passed.)

Mark anything they confirm as `done` via
`brain tasks complete <H##>` BEFORE generating the agenda, so the
agenda reflects reality and doesn't list already-completed work in
the Suggested order. Items the user explicitly says they haven't
done stay in the agenda even if "late".

Skip this question entirely if it's still early in the day (before
~8 AM) — none of the habits would have fired yet, so there's
nothing to ask about.

### Updates vs regeneration — DEFAULT TO UPDATE

The collaborative generation flow above (asking questions,
asking about already-completed habits, treating it as a working
draft) **only runs when the agenda for today does not yet exist**
— i.e. there's no `/tmp/<YYYY-MM-DD>.md` for the target date.

**Once today's agenda file exists, every change is an UPDATE, not
a regeneration.** This is the dominant case during the day.

- "update the agenda" → update.
- "mark T## done" / "I finished X" → update.
- "drop T##" / "push Y to tomorrow" → update.
- "the agenda needs to reflect …" → update.
- Anything implicit (user marks tasks done, defers things, etc.)
  → update.

In update mode:

- **Do NOT ask scoping questions.** No "which MIT first", no
  "have you done any habits yet" — that ship sailed at
  generation time. The agenda is already framed.
- **Do NOT walk the user through choices** they already made
  during generation. Just rewrite the file per the "Update means
  rewrite, not patch" rule.
- **Regenerate the PDF only if one already exists** at
  `$AGENDA_DIR/agenda-<today>.pdf`. If a PDF is on disk it must
  stay current — regen it. If none is on disk, skip the PDF
  step; a CSV mutation isn't a request for a fresh printout
  (per the operating-principle-7 carve-out).
- If a true scoping decision genuinely surfaces mid-day (e.g.
  three new p0 tasks just landed and one has to be cut), ask a
  single targeted question — but treat that as the exception,
  not the rule.

**Only re-run the full generation flow if the user explicitly
asks for it** — phrasings like "regenerate the agenda", "start
the agenda over", "rebuild from scratch", "let's redo the day's
plan from the top". A bare "update" never triggers regeneration.

## Agenda generation — phased procedure

Building an agenda touches a lot of rules at once (anchors,
work/personal partitioning, MITs, hard deadlines, chunked tasks,
two-page PDF cap, etc.). Run through these phases in order. Each
phase has a single job. Skipping a phase or running them out of
order is the most common source of broken agendas.

### Phase 1 — Load state (read-only)

1. Note wall-clock time (`date`).
2. Read `<brain>/tasks/tasks.csv` — keep rows where `status` is neither
   `done` nor `backlog`, AND (`start_date` is empty OR `start_date <= today`).
   A `backlog` task is parked indefinitely (see "Backlog status") — it never
   appears on an agenda until restored. A task whose
   `start_date` is in the **future** is *not yet actionable* — exclude it
   entirely (no bucket, no MIT callout, no Suggested order, no Cut order).
   This is the `is_visible_today` rule (see references/schema.md): a
   `start_date` deliberately parks a task until a date the user can't or
   won't start before. It reappears in the agenda automatically on the day
   `start_date <= today`. (Its `due_date` can be well past `start_date` —
   the task simply isn't surfaced anywhere until its start day arrives,
   even if the deadline is sooner.)
3. Read `<brain>/tasks/habits.csv` — keep rows where (`status != done`)
   OR (`status == done` AND `completed_date == today`).
4. Bucket the tasks:
   - **MITs**: `mit` in `task_type`.
   - **Hard deadlines today**: `hard_deadline == true` AND
     `due_date <= today`.
   - **Hard deadlines this week**: `hard_deadline == true` AND
     `due_date <= today + 7d` (not yet today).
   - **In-progress**: `status == in_progress`.
   - **Due today**: `due_date == today`.
   - **Chunked-task families**: rows whose name matches the
     `(i/N)` chunk pattern. Compute the per-family minimum chunks
     today per the Chunked tasks section.
5. **Fetch today's calendar busy blocks.** If `calendar_id` is
   configured, load the day's busy blocks for `<TARGET_DATE>` from it
   and produce the list of immovable time blocks for today. The
   source, query, and filter specifics are personal — see
   [Calendar busy blocks](#calendar-busy-blocks) and its `todo:calendar`
   extension. Without `calendar_id`, skip this step.

### Phase 2 — User confirmations (batched)

Only relevant on first-build (no `/tmp/<today>.md` yet). On
updates, skip — those questions were already settled at
generation. Use `AskUserQuestion` (per operating principle 12),
batched.

1. **Already-done habits.** If wall-clock is past ~8 AM AND any
   habits have `ideal_time < now` AND `status == not_started`,
   ask once which ones the user already did off-system. Mark
   those done via `brain tasks complete <H##>` BEFORE Phase 3 so they
   drop out of "Today's habits".
2. **Hard-deadline still real?** For every hard-deadline task with
   `defer_count >= 1`, ask whether the deadline still holds (per
   operating principle 11). Apply the user's call (keep / push /
   drop the hard flag) before Phase 3.
3. **Any other genuinely scoping question.** Cap at 2-3
   questions total, batched. Examples in "First agenda
   preparation". When in doubt, don't ask.

### Phase 3 — Apply daily anchors (fixed time blocks)

Mark fixed slots as occupied before any task placement:

- **Your personal daily anchors** — recurring fixed blocks (a lunch
  window, a standup, a workout, a school run, etc.). These are personal;
  a `todo:anchors` extension defines the specific set and their times.
  Without one, core has no hardcoded anchors.

<!-- brain:ext todo:anchors -->

- **9:00 PM** — agenda hard stop (no Suggested-order items at or
  after 9 PM; surface as tradeoff if work doesn't fit)
- An optional **work-hours cutoff** (see [Work-hours cutoff](#work-hours-cutoff-optional-personal)
  and its `todo:cutoff` extension), if configured.
- **Calendar busy blocks** loaded in Phase 1 (meetings, appointments,
  focus time, "Busy" holds — anything the configured `calendar_id`
  marks busy). Each block is an immovable slot for its full
  `start`-`end` duration — no task or habit may be scheduled inside it.
  See [Calendar busy blocks](#calendar-busy-blocks) for the rendering
  convention.

### Phase 4 — Partition tasks by work-vs-personal window

Classify each non-habit task as **work** or **personal**:

- **Which `task_type`s are "work" vs "personal" is personalization.**
  Read the mapping from the `todo:anchors` extension (it declares which
  tags are work-flavored, which are personal, and how mixed/flag tags
  like `mit`/`needs_attention` resolve). Without an extension, treat all
  tasks the same (no partition) and place purely by priority/deadline.

Assign each task a default window (when a partition is defined):

- **Work tasks** → the work window. Within it, MITs and hard-deadlines
  pull earliest morning slots; lower-priority fillers go later.
- **Personal tasks** → the pre-work or post-work blocks. Pick the half
  that fits the task's energy better — high-energy personal items
  (workout, real errands) fit the evening block; quick admin fits the
  morning block.

**Habits override the partition.** A habit with an `ideal_time` inside
the work window stays there regardless of work/personal nature; the
`ideal_time` is the user's stated preference. Habits without an
`ideal_time` fill in naturally around the partition.

**Personal-in-9-5 carve-out.** A personal task may land inside
9-5 when:

- It's externally constrained (doctor's appointment only
  bookable mid-day, a 2 PM dad-call he's available for).
- Out-of-hours blocks (7-9 AM + 5-9 PM = 6 hours total) genuinely
  cannot fit everything personal. In that case, prefer pushing
  the lower-priority personal items to tomorrow rather than
  invading the work block. Surface the tradeoff per operating
  principle 10.

When you do place a personal task in 9-5, **state the reason
inline** in the Suggested-order line (e.g. `~2:00 PM | T### —
dad call (only window he's free)`) so the user can see why the
partition was broken.

### Phase 5 — Place anchors-first, MITs-next

Within each window, place items in this priority:

1. **Mandatory habits in their natural slot:** Pray (morning),
   Stretch+Meditate (paired, prefer 9-9:30 AM block).
2. **MITs and hard-deadline-today tasks** in the morning work
   block (9:00 AM-12:00 PM) when feasible. The morning work
   block is often the highest-leverage window of the day.
3. **Chunked-task minimums** per the Chunked tasks section's
   "Surface the smallest number of chunks" rule.
4. **Code-work minimum (operating principle 13):** ensure at least
   2 items from the code-work pool (coding tasks + `Review PR: …`
   tasks) are placed, PR reviews slightly ahead of equal-priority
   coding work. Drop below 2 only when fitting them would make a
   higher-priority / hard-deadline-today item unachievable — and
   then say so per operating principle 10.

### Phase 6 — Fill remaining items

1. Walk the morning work block (9-12), filling work tasks by
   priority (`p0 > p1 > p2 > p3`) and decreasing duration.
2. Walk the afternoon work block (1-5), same priority order.
3. Walk the 7-9 AM personal block (quick admin / setup tasks).
4. Walk the 5-9 PM personal block (workout, family, errands,
   late readings).
5. Habits at their `ideal_time`, in their existing slot.
6. **Stop placing items at 9:00 PM.** If something didn't fit,
   it falls out of today's plan — surface that as a tradeoff
   (operating principle 10), don't quietly drop it.
7. **Empty time at the end of the day is the correct output —
   do not fill it.** If all today-eligible tasks and habits are
   placed and the schedule still has free time before 9 PM, the
   schedule is *done*. Do NOT propose pulling at-risk work
   forward, chipping at hard-deadline tasks earlier than the
   chunked-task minimum, or otherwise "getting ahead" to claim
   the empty slot. Finishing the planned day early is a feature
   of being on schedule and protects the user's mental health,
   not slack to be reclaimed. Report free time as free time;
   don't suggest candidates to fill it. (Exception: the user
   explicitly asks "what else can I do?" — then it's fair to
   propose pulls.)

### Phase 7 — Build the Cut order (≤ 5 items)

From the Suggested order, pick the 5 most-droppable items, in
the order you'd drop them. **Hard cap: 5 items.**

Cut-order eligibility (in priority of "drop first"):

1. Chronic-ignore items just revived or started — they aren't
   load-bearing today.
2. p2/p3 tasks with no hard deadline.
3. Big undefined-scope tasks (no duration estimate, vague
   names) — risk of swallowing the day.
4. Tasks deferred 2+ times that the user clearly avoids.
5. Optional habits (Workout, low-priority readings) — only as
   last resort; habits are usually not cut.

**Never include in Cut order:**

- MITs.
- Hard-deadline-today tasks.
- Mandatory daily habits (Pray, Meditate, walks, lunch,
  mandatory readings).

### Phase 8 — Render to `/tmp/<TARGET_DATE>.md`

Write the markdown file per operating principle 8's section
contract:

1. Title: `# YYYY-MM-DD — <weekday> agenda`
2. `**Load:** …` one-liner
3. `**Bottom line:** …` one-liner (1-2 sentences max)
4. `## ❗ MITs — if only these get done, today is a win`
5. `## Suggested order` (numbered checkboxes, scheduled
   non-task items unnumbered)
6. `## Cut order if the day slips` (numbered, ≤ 5 items, no
   checkboxes)
7. `## 🔁 Today's habits` (2-col table, ◻/✅ glyphs)
8. `## ✅ Completed today` (2-col table, omit when empty)

### Phase 9 — Generate PDF + verify 2-page cap

```
rm -f $AGENDA_DIR/agenda-<TARGET_DATE>.pdf
markdown-to-pdf /tmp/<TARGET_DATE>.md \
    --out $AGENDA_DIR/agenda-<TARGET_DATE>.pdf --agenda
python3 -c "from pypdf import PdfReader; print(len(PdfReader('/agenda-<TARGET_DATE>.pdf').pages))"
```

If body exceeds 2 pages: escalate `--font-shrink 1`, then `2`,
then `3`. If still spilling at `--font-shrink 3`, trim names per
the priority list in operating principle 8 (habit cell names →
suggested-order names → MIT names → split the habits table).

### Phase 10 — Work-hours cutoff (optional)

If a work-hours cutoff is configured (the `todo:cutoff` extension),
apply it here — decide whether the Suggested order pushes work-flavored
tasks past the cutoff and run whatever tracking/warning the extension
defines. Without a `todo:cutoff` extension, skip this phase.

---

## Agenda rules and constraints (daily anchors)

Hard daily anchors the agenda MUST respect:

- **Day start.** The agenda spans the user's waking day (a morning
  start through the evening hard stop below).
- **Your personal anchors + work window.** Fixed recurring blocks and
  the work-vs-personal-hours partition (which `task_type`s are
  "work-flavored" and where each kind is allowed to land) are personal;
  the `todo:anchors` extension defines them. **Exception: habits.** A
  habit whose `ideal_time` falls inside a work window stays at its
  `ideal_time` — the stated preference overrides any partition.
- **Calendar busy blocks + optional work-hours cutoff** — see the
  respective sections; both come from extensions if configured.
- **9:00 PM — agenda hard stop.** Nothing on the printed
  Suggested order at or after 9:00 PM. The last actionable item
  must start at 8:59 PM or earlier and finish by 9:00 PM. Items
  the user genuinely wants to do between 9 PM and bed (a book, a
  paper journal, a meditation, etc.) are handled by the user
  off-agenda — they're calm, screen-free, and don't need a
  printed slot. **Treat 9 PM the way 7 PM treats work:** anything
  scheduled past it is a bug unless the user explicitly asks for
  it. If something genuinely cannot land before 9 PM, surface
  the tradeoff and defer rather than overstuffing.
- **10:00 PM — day ends. In bed.** This is the user's bedtime
  anchor; the agenda already stops at 9 PM, so this line is
  here as context, not as a scheduling constraint.
- **Reading habits** — three daily reading sessions exist
  (morning / mid-day / afternoon, ~10 min each). The agenda MUST
  fit at least 2 of the 3.
- **Social media harvesting (10 min) — MANDATORY every day.**
  Treat it like the reading sessions: a non-negotiable daily slot
  that the agenda must include. Fits naturally into a slack
  window between MITs or after lunch.
- **Pray (H8) — MANDATORY every day, prioritized for the
  morning.** Not optional, not deferrable to "if time permits."
  Schedule it in the morning block of the Suggested order before
  any work blocks start. If the agenda is being built mid-day
  and Pray hasn't happened yet, place it in the next available
  slot — but the default expectation is "first thing."
- **Meditate (H5) — MANDATORY every day, any time of day.**
  Not optional. Strongly prefer placing it immediately after
  Stretch (H22/H30) in the Suggested order — the two are
  cadence-coupled (stretch first, then meditate while the body
  is settled). If Stretch is already done off-system, schedule
  Meditate solo in any open slot. Meditate is also a perfect
  fit for the 9:00-10:00 PM power-down window if it hasn't
  happened by then.
- **Buffer time** — build in slack. Don't pack the day
  wall-to-wall.

### Calendar busy blocks

If `calendar_id` is configured (`brain config get calendar_id`), pull the
day's busy blocks from that calendar and treat them as **immovable anchors**,
just like any hardcoded anchor. Render each busy block in chronological
position in the Suggested order as a **scheduled non-task item** —
`<start-time> | <event summary> (<duration>m)`, with no number, no `- [ ]`
checkbox, no list marker — and never overlap a task's `[start, start+duration]`
window with one. Per operating principle 7 ("update = rewrite, not patch"),
**re-query the calendar on every agenda build/update** rather than
forward-copying blocks. Without `calendar_id`, skip calendar integration
entirely.

The source-specific details — which calendar API/tool to call, the exact
busy-block filter (status/transparency/declined/event-type), which shared
calendars to ignore, and dedup against hardcoded anchors — are personal; a
`todo:calendar` extension supplies them.

<!-- brain:ext todo:calendar -->

### Work-hours cutoff (optional, personal)

Core scheduling has no work-hours boundary. If you want a soft cutoff (e.g.
"no work-flavored tasks scheduled past a certain hour", with streak tracking
and a warning ladder), a `todo:cutoff` extension defines it — including which
`task_type`s count as "work" and how late is too late.

<!-- brain:ext todo:cutoff -->

**Why agenda lives in /todo (not its own skill).** Agenda
generation reads directly from `tasks.csv` / `habits.csv` and is
tightly coupled to task state (MITs, hard deadlines, defer counts,
suggested order). Splitting it into a separate skill would either
duplicate the task-reading logic or force /agenda to re-load /todo
every time. Keeping it inside /todo means one skill load covers
both. Revisit if /todo grows past ~600 lines and the agenda
section becomes the bulk of it.

## Commands

Full catalog: [references/commands.md](references/commands.md). The
load-bearing ones:

- **`/todo what`** — what should I work on right now?
- **`/todo agenda`** — day briefing. Always written to
  `/tmp/<YYYY-MM-DD>.md` (see operating principle 7). The user
  opens it with the `agenda` zsh function.
- **`/triage`** — past-due triage. Handled by its own skill at
  `../triage/SKILL.md`. (The old
  [triage-heuristics.md](references/triage-heuristics.md) is still
  here for historical reference, but `/triage` is the canonical
  entry point.)
- **`/todo turn-into-project <task>`** — convert oversized task to
  project + sub-tasks. See
  [task-project-link.md](references/task-project-link.md).
  Every conversion (whether triggered here, by `/triage`, or any
  other flow) MUST end with: (a) each proposed sub-task written as
  a row in `tasks.csv` with `project=<slug>`, (b) the project's
  `.METADATA.json:tasks[]` populated with those `task_id`s, and (c)
  any sequential dependencies between sub-tasks encoded explicitly
  via `blocked_by`. Don't leave the dependency structure in the
  user's head.
- **`/todo done|defer|add|remove|list`** covers the usual CRUD. After resolving any
  links or confirmation needed for removal, `/todo remove` must execute
  [scripts/remove_task.py](scripts/remove_task.py) so deletion crosses the
  config-aware task-store guard. Never delete a CSV row directly.
  Removing a **habit** additionally requires `--habit`, because deleting a habit
  row destroys the entire recurring chain including every future occurrence. The
  script refuses a habit needle without that flag, so only an explicit "retire
  this habit" request may pass it — confirm with the user first, and prefer
  [scripts/defer_habit.py](scripts/defer_habit.py) when they only want the next
  occurrence pushed out. See [Habits are never cleanup fodder](#habits-are-never-cleanup-fodder).
- **`/todo chronic`** — list chronically-ignored tasks (the same set
  that `/triage` Step 7 sweeps). Backed by
  [scripts/find_chronic_ignored.py](scripts/find_chronic_ignored.py).
  Useful for ad-hoc deadwood inspection without running a full
  triage pass.
- **`/todo touch <task>`** — bump `last_touched` to today without
  changing anything else. Use when the user wants to explicitly
  acknowledge a stale task ("yes I still care, leave it") so it
  won't reappear in chronic-ignore for another 21 days. Backed by
  [scripts/touch_task.py](scripts/touch_task.py).
- **`/todo backlog <task>`** — park a task indefinitely (see
  [Backlog](#backlog)). Backed by
  [scripts/backlog_task.py](scripts/backlog_task.py). `--restore` brings
  one back to active.
- **`/todo restore <task>`** — pull a task out of the backlog
  (`backlog_task.py <task> --restore`); set a fresh `due_date`/`priority`
  after.
- **`/todo reindex`** — apply automation rules + cleanup. Mirrors what
  `/second-brain reindex` runs for tasks.

## Managed triage rows

Brain may maintain two protected habit chains, identified by
`system_key=brain.triage.daily` and `system_key=brain.triage.weekly`. The
system key is authoritative even when a visible habit name is changed. While
`brain config get enable_triage_habits` is `true`, managed triage rows cannot be removed, completed, revived, or skipped through ordinary `/todo`, task, or habit mutation paths. Do not work around the guard by editing CSV directly. The `/triage` skill owns its narrow completion helper.

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
     2. Where you can help directly right now — drafting, research,
        code, planning, summarizing — so the first chunk gets knocked
        out together in the conversation.
- Respect the hard prohibitions and personal/executive-assistant mode
  while doing this: be blunt, make obvious calls yourself, and flag
  anything the user must do themselves (e.g. Vercel/Supabase-prod
  actions) rather than doing it for them.

## Reindex / automation rules

See [references/sync-rules.md](references/sync-rules.md). Both this
skill and `/second-brain` execute the same rules via
[scripts/apply_sync_rules.py](scripts/apply_sync_rules.py) +
[scripts/cleanup_done_habits.py](scripts/cleanup_done_habits.py).

**Never ask the user whether to run `/todo reindex` — just run it when
you believe it's necessary.** Reindex is a safe, idempotent
reconciliation (apply rules + cleanup), not a destructive op, so it
needs no confirmation. Run it without asking whenever the system
signals it's needed — most commonly when a mutator path prints a
`run /todo reindex to refresh` reminder (e.g. after `brain tasks complete`
or `defer_task.py` on a project-linked task), or any time you've made
changes that could leave the task↔project link, habit table, or
automation-rule state stale. Report what reindex did in passing; don't
gate it behind a question. (This is an explicit standing instruction
from the user.)

## Habit recurrence (anchor-to-due with catch-up)

When a habit is marked done, a new instance is spawned with
`due_date = original_due + N × interval`, where N is the smallest
integer that makes the result **strictly after today**. A
"Monday-weekly" habit stays on Mondays even if you complete it
Tuesday — and a stale habit (e.g. 8 weeks old) lands on the next
future Monday, not in the past. Math is done by
[scripts/next_habit_occurrence.py](scripts/next_habit_occurrence.py)
— LLMs are bad at calendar arithmetic, always use the script.

Completed habits stay in habits.csv for 7 days then get pruned by
`cleanup_done_habits.py` during sync. Managed completed occurrences use this
same retention rule only while managed triage habits are enabled. Cleanup does
not perform the feature-off purge; the transactional Brain reconciler owns
that coupled config/data change. That's your audit trail.

## Habits are never cleanup fodder

**A past-due habit is not a stale task, and no cleanup pass may ever delete,
drop, defer, backlog, or purge one.** A habit that is weeks past due is the
normal resting state of a habit the user hasn't gotten to — the pending row *is*
the habit. Deleting that row destroys the whole chain: there is no other record
of the cadence, so every future occurrence disappears with it and the habit
silently stops existing.

The rules, in force for every flow in this skill and for `/triage`:

- **Only `status=done` habit rows are ever removed automatically**, and only by
  `cleanup_done_habits.py` after its 7-day retention window. A
  `not_started` habit row is never removed automatically, at any age.
- **Never route a habit through a task-cleanup script.** `remove_task.py`
  refuses a habit needle unless given `--habit`;
  `backlog_task.py` refuses habits outright. Do not work around either.
- **Retiring a habit is an explicit user decision**, never inferred from the row
  being old. Ask, and only then pass `--habit`.
- **To get a past-due habit out of the way, move it, don't kill it**: `brain
  habits skip` (cadence-aware) or `defer_habit.py` push the occurrence forward
  and keep the chain alive. Prefer these in every case where a task would have
  been dropped.
- **A lapsed chain is repaired, not recreated.** If every row for a habit is
  `done` and nothing is pending, run `brain habits revive <fuzzy>`; don't
  hand-add a replacement row.

## Skipping a habit

"Skip habit X" / "skip X today" / "we can skip X" means the user is
opting out of a habit **for today**. What that does to the row depends
on the habit's cadence — always run `brain habits skip`, which decides
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
  handled" — the same thing marking it done means. No separate defer.
- **Non-daily habit** (weekly, monthly, every-N-days, …) → **do NOT
  mark it done; defer its `due_date` to tomorrow** (today + 1 day).
  Skipping a non-daily habit is, by default, a **one-day defer** — the
  skipped instance simply isn't done and reappears tomorrow.
- **"Skip until a certain day"** (`--until YYYY-MM-DD`, either
  cadence) → defer the `due_date` to that day, never marking it done.
  Must be strictly after today.

This is distinct from [`defer_habit.py`](scripts/defer_habit.py), which
skips a **whole recurrence interval** (a weekly habit jumps to next
week). `brain habits skip` is the "not today" lever; `defer_habit.py` is
the "not this cycle" lever.

Like `brain tasks complete`, `brain habits skip` mutates `habits.csv`
natively and does **not** touch the agenda file — the next agenda build
re-derives habit state from the CSV, so a skip is reflected there
automatically (a PDF already on disk refreshes on the next agenda touch).

**Skipping daily triage is just this rule applied to a daily habit.**
The "Morning Triage" habit recurs daily, so `brain habits skip "Morning
Triage"` marks today's occurrence done — which is exactly what stops
the `tasks` TUI from nagging (it gates on that habit being `done`).
There is no longer a special-case skip path for daily triage; it's the
general daily-habit skip. See /triage SKILL.md.

## Chunked tasks

A **chunked task** is a single atomic unit of work split into
multiple equal-duration sessions because doing it in one sitting is
unrealistic — e.g. "draft the whitepaper" as five 30-minute blocks
rather than one 2.5-hour block. Chunks are *not* sub-tasks. They are
the same task, time-boxed across sessions.

### Chunks vs projects — the distinction matters

- **Project conversion** (`/todo turn-into-project`) decomposes work
  into *different* atomic sub-tasks (research → outline → draft →
  edit → publish). Each sub-task has its own scope and name.
- **Chunking** repeats the *same* atomic work across N sessions. All
  chunks share a name (modulo the `(i/N)` suffix) and a duration.

If the work needs decomposition, use a project. If the work just
needs to be split over multiple sittings, chunk it. When in doubt,
chunking is lighter weight — easier to undo, no project folder,
no `.METADATA.json` plumbing.

### Creating chunked tasks

User phrasings to recognize:

- "split this into 5 chunks of 30 minutes"
- "chunk into 4 sessions of 45 min"
- "break this into 3 thirty-minute blocks"
- "create '<task>' as five 30-min chunks due Friday"

Route these to [`add_task.py`](scripts/add_task.py) with the
`--chunks N` and `--duration M` flags. The script handles the
naming, ID issuance, sequential `blocked_by`, and inheritance.

### Chunked-task invariants (enforced by `add_task.py --chunks`)

- **Naming:** `<base> (i/N)` for i in 1..N. The `(i/N)` parenthesized
  fraction at the end of the name is the canonical chunk marker —
  scripts parse it via [`_csvlib.parse_chunk_name`](scripts/_csvlib.py).
  Never use a different format (e.g. `<base> [i of N]`); the
  detection logic only recognizes `(i/N)` at end-of-name.
- **Same `due_date`** across all chunks. The deadline is for the
  whole work, not per-chunk.
- **Same `priority`, `task_type`, `project`, `energy_level`,
  `context`, `see_also`, `notes`** — they describe the same work.
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

`is_visible_today` does NOT filter by `blocked_by`, so all chunks
will show up in raw views. **The LLM is responsible for the
"surface minimum chunks to land on time" rule when rendering an
agenda or `/todo what`.** The rule:

> Surface the **smallest number of chunks** that need to be
> completed today for the full chunked task to finish by its
> `due_date`, factoring in the day's remaining capacity (daily
> anchors, other tasks, MITs, hard-deadlines, habits).

Procedure when an agenda touches a chunked-task family:

1. Identify the family: same `<base>`, same total `N` (parse via
   `parse_chunk_name`).
2. Count remaining work: `R = N - (# chunks done)`.
3. Count days remaining until `due_date` (inclusive of today).
4. Compute the **minimum chunks needed today**:
   `ceil(R / days_remaining)`. If `due_date` is today or past, it's
   `R` (everything must land today).
5. Walk forward from the lowest-numbered un-done chunk; surface
   that many chunks in the Suggested order.
6. **If that count doesn't fit** alongside other hard-deadline /
   p0 / MIT obligations, do NOT silently overstuff — surface the
   tradeoff to the user per operating principle 10. Common
   resolutions:
   - Defer the chunked task's `due_date` and recompute.
   - Drop a competing lower-priority task.
   - Accept the chunked task will be late.
   Make a recommendation; don't dump the choice on the user. Ask
   for confirmation only when the right call genuinely isn't
   obvious.
7. Remaining un-done chunks beyond today's count stay in tasks.csv
   but do NOT appear in the agenda's Suggested order / MIT callout
   / Cut order. They're scheduled implicitly for later days.

When chunk i is marked done mid-day and you're updating the
agenda (operating principle 7's auto-update checklist), re-run the
above so the next chunk (which just inherited the MIT tag) lands
in the Suggested order in its place.

### Defer cascade

`defer_task.py` is chunk-aware. When the deferred row is part of a
chunk family, later chunks whose `due_date` would otherwise become
earlier than the deferred chunk's new date are **automatically
pushed forward** to preserve the family order. This is handled by
[`_csvlib.cascade_chunk_dates_forward`](scripts/_csvlib.py).

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
- **Single-chunk "chunks":** `--chunks 1` is rejected — that's just
  a regular task. Use plain `/todo add`.

## Backlog

The **backlog** is for tasks the user wants to keep but isn't going to
act on for the foreseeable future — not abandoned (`/todo remove` is for
that), just parked. It's the pressure-release valve for the chronic-defer
problem: instead of a task slipping week after week and nagging every
triage, it goes to the backlog and goes quiet.

**Semantics (enforced by [scripts/backlog_task.py](scripts/backlog_task.py)):**
a `status = backlog` task has its `due_date` and `start_date` **cleared**
(a parked task has no schedule; `hard_deadline` and `waiting_since` are
cleared too), is stamped with `backlogged_date = today`, and is **hidden
from everything active** — `is_visible_today` is false, so it never shows
in `/todo what`/`today`/`list`, never lands on an agenda, and is never
surfaced by the at-risk or chronic-ignore scans. It resurfaces only in the
**monthly** triage's backlog-review.

**Entering the backlog:**

- `/todo backlog <task>` directly, or the **"Move to backlog"** action
  during `/triage` (past-due, at-risk, and chronic-ignore walks all offer
  it).
- **Suggest it proactively when `defer_count >= 4`.** A task deferred four
  or more times is one we keep avoiding — offer "move to backlog instead?"
  rather than deferring a fifth time. (This is a personal-assistant
  trigger; see that section.)

**Restoring:** `/todo restore <task>` (or `backlog_task.py <task>
--restore`) flips it back to `not_started` and clears `backlogged_date`;
set a fresh `due_date`/`priority` afterward. The monthly backlog-review is
the main path back (see `/triage`).

**Implicit revive = silent dedupe.** If you re-create a task by hand after
parking the original (an active task whose `created_date` is later than the
backlog task's `backlogged_date`, same name), the monthly triage's
[scripts/dedupe_backlog.py](scripts/dedupe_backlog.py) treats your manual
re-creation as the intended revive and **silently deletes the now-duplicate
backlog row** — no prompt, no report.

**Auto-purge at 6 months (silent).** A backlog task whose `backlogged_date`
is **more than 6 months ago** (≥6 months + 1 day) is **deleted outright**
by [scripts/purge_old_backlog.py](scripts/purge_old_backlog.py), which
`/triage` runs every pass. This is deliberately silent: **never warn the
user a backlog item is nearing deletion, and never tell them which items
were deleted.** Six months parked = forgotten = fine to forget forever.
The one bookkeeping exception: if a deleted task belonged to a project
(active or archived), the purge leaves a breadcrumb in that project's
`.METADATA.json` (`deleted_backlog_tasks[]`) and `notes.md` so a future
un-archive knows tasks used to exist.

**Backlog ↔ projects.** When you backlog a task that belongs to a project,
you MUST ask the follow-up (via `AskUserQuestion`):

- **Backlog a project-linked task** → ask: "backlog just this task, or the
  whole project?"
- **Backlogging would empty the project** (it was the last active task, or
  the user chose "whole project") → ask: "archive the project too?" If yes,
  archive it per `/second-brain` conventions (move to `archive/` mirroring
  its path, set `.METADATA.json:status`) AND backlog every one of its
  tasks.
- **"Backlog this project"** (user names a project, not a task) → interpret
  as: **archive the project + move all its tasks to the backlog.** Confirm
  once, then do both.

Keep the brain project and tasks.csv consistent throughout — same rule as
project conversion (see [task-project-link.md](references/task-project-link.md)).

**Backlog ↔ external tracker.** The "Backlog ⇒ no dates" invariant applies
to a linked tracker too: parking a task should leave no live deadline on the
tracker side either. The specifics (how to clear a linked issue's date, and
how `/triage` reconciles a tracker's own "Backlog" status) are part of the
tracker workflow — see the `todo:linear` extension point below.

<!-- brain:ext todo:linear-backlog -->

## External issue tracker (optional)

Tasks can carry an optional link to an external issue tracker in the
`linear_issue` column, managed by the local-only scripts
[`set_linear_issue.py`](scripts/set_linear_issue.py) and
[`list_linked_tasks.py`](scripts/list_linked_tasks.py). Core treats this as
inert metadata: an empty `linear_issue` is always fine, and nothing here talks
to any external service.

The **workflow** for syncing tasks with a tracker — filing a task as an issue,
keeping open/closed + `due_date`/`priority`/`title` in sync both ways, mirroring
projects, and the reverse reconcile — is personal (it depends on which tracker
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
- `/todo today` returns >5 tasks → suggest `/todo plan-day` to
  order them.
- Triage time (10+ past-due tasks) → suggest `/todo triage`.
- **A chronically-ignored task surfaces inline** — i.e. a row whose
  `today - last_touched >= 21d` (or `>= 14d` while `status = in_progress`)
  appears as a candidate in `/todo what`, `/todo today`, `/todo list`,
  project status, or any other surfacing flow. Flag it explicitly:
  > "T### '<name>' hasn't been touched in 28 days. Drop, revive,
  > start now, or convert to a project?"
  Don't silently recommend it as the next action — a stale top
  candidate is itself a signal worth pausing on.
- **5+ chronic-ignore hits at once** (run
  `python3 ~/.agents/skills/todo/scripts/find_chronic_ignored.py --count`
  to check) → suggest `/triage daily`; the chronic-ignore sweep
  (Step 7) is the right pass for clearing a backlog of deadwood.

## When in doubt

- Read [commands.md](references/commands.md) for the full command map.
- Read [task-project-link.md](references/task-project-link.md) for
  anything touching projects.
- Run `apply_sync_rules.py` (dry-run) before mutating to see what
  state the system is in.
- Ask the user. This is their task system.
