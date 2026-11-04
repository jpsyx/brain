---
name: triage
description: Use when the user asks to triage tasks, run "morning triage", run weekly review / in-basket processing, clean up past-due tasks, or asks "what's slipping?" / "what's about to slip?" / "what's rotting?" - supports daily (past-due cleanup + at-risk preview + chronic-ignore sweep) and weekly (scratch + capture/ in-basket processing + monthly backlog review) modes.
---

# triage

Throughout this skill, `<brain>` is the selected workspace root in
`BRAIN_ROOT`. Every task and habit operation below is a `brain` subcommand -
this skill ships no scripts. Run them inside the Brain workspace/actor
environment so they act on the selected workspace; `-w <name>` picks another.
Preserve each row's `assigned_to` unless the user asks
for explicit reassignment through the validated `/todo assign` path.

## Capture-only requests

For a request limited to processing capture, delegate the named selection to
[/process-capture](../process-capture/SKILL.md) and stop after its report.
Do not run weekly review or complete the weekly habit for that request.
Explicit weekly review still runs the full weekly procedure, including both local
in-baskets, monthly checks, hooks, and feature-gated managed completion.

## Triage never touches habits (hard invariant)

**Every cleanup pass in this skill operates on `tasks.csv` only. Habits are
excluded from triage, always.** Read `<brain>/tasks/habits.csv` for exactly two
reasons: the weekly-vs-daily routing check, and marking this run's own managed
Morning Triage / Weekly in-basket occurrence done (Steps 9 and weekly Step 3).
Nothing else in triage may read it, and **nothing in triage may ever delete,
drop, defer, backlog, purge, or reschedule a habit row.**

This is not a style preference - it is a data-integrity rule. A past-due habit
is the normal resting state of a habit the user hasn't gotten to; the pending row
*is* the habit, and it is the only record of the cadence. Dropping it as if it
were a stale task destroys the entire chain, silently taking every future
occurrence with it.

Concretely, for every pass below:

- **Past-due triage (Steps 1–4), the at-risk scan (Step 6), and the
  chronic-ignore sweep (Step 7) enumerate `tasks.csv` rows only.** A habit must
  never appear in a group, a count, a bulk operation, or a per-task prompt. If a
  past-due list you assembled contains an `H###` row, the list is wrong - rebuild
  it from `tasks.csv`.
- **Drop applies to tasks only.** `brain tasks remove`
  refuses a habit needle unless given `--habit`; triage must **never** pass that
  flag, under any circumstance, including a direct user instruction mid-triage
  (send them to `/todo remove` for that, so retiring a habit is a deliberate act
  outside a bulk cleanup).
- **Proposed defers and spreads apply to tasks only.** Do not use `brain tasks defer` or
  `brain habits defer` on a habit during triage. A habit the user wants out of the
  way today is handled by `/todo`'s cadence-aware `brain habits skip`, after
  triage, at their explicit request.
- **Step 0's backlog auto-purge only touches `status=backlog` rows in
  `tasks.csv`.** Habits have no backlog state and are not eligible.
- **Never "clean up" habits as a courtesy.** Do not offer it, do not batch it
  into a summary action, and do not treat a long list of past-due habits as a
  problem to solve. Mention it at most as an observation, and only when the user
  asks about habits.

The one habit mutation triage performs is completing its *own* managed
occurrence via the protected command in Step 9 / weekly Step 3, which advances
the chain rather than ending it. See
[/todo "Habits are never cleanup fodder"](../todo/SKILL.md#habits-are-never-cleanup-fodder).

## Establish "today" from the system clock FIRST (before anything else)

Every triage action is date-sensitive. **Before doing anything, run
`date +%F` and treat its output as the authoritative "today."** Do NOT
trust the `currentDate` / "Today's date is …" value from your session's
opening context - it is frozen at session start, and this transcript can
stay open for **days** (the user may not close the session overnight). A
stale cached date silently marks the **wrong day's Morning Triage habit**
done (e.g. a "skip triage" honored against yesterday's occurrence while
today's stays pending) and mislabels everything downstream. Re-run
`date +%F` at the start of the routing check, the skip flow, and Step 9,
and resolve the habit row by *that* date. Shell clock wins over context,
always. (Mirror of /todo SKILL.md operating principle 5.)

## Check whether managed triage habits are enabled

At the start of every invocation, after resolving today's date, run:

```
brain --workspace "$BRAIN_WORKSPACE" config get enable_triage_habits
```

Keep that result for this invocation. When it is `false`, still run the full manual triage workflow. Only the managed habit routing and mutation steps are skipped. Task cleanup, in-basket processing, summaries, extension steps, and the optional background completion signal remain unchanged.

## Personal-assistant mode

This is a personal/executive assistant skill. Load who you're assisting:
run `brain persona list`. It prints one block per workspace member, keyed by
user ID. Select the person identified by `BRAIN_ACTOR_ID`, not the `(this machine)`
marker. Read the other blocks only to understand who else shares this workspace. Honor their `role`/`works_for` (both may be
unset - then keep the framing neutral). **Top priority is saving the user's
time.** Propose, don't ask: decide a default for every item and let the user
correct it in one answer. Be blunt. Make obvious decisions. Ask the user only
when you genuinely can't proceed - and when you ask, batch questions and keep
them short. Daily triage has a hard [question budget](#daily-question-budget).

Sibling skills: /todo (tasks), /agenda (day planning), /projects, /process-capture, /organize, /brain-sync (knowledge workflows). All engage the same assistant mode.

## Modes

This skill has two distinct workflows:

- **`/triage daily`** (alias: `/triage morning`) - past-due task triage. The fast-paced "morning triage" pass over `tasks.csv`. See [Daily triage](#daily-triage) below.
- **`/triage weekly`** - in-basket processing across your brain's scratch notepad and its `capture/` in-basket (plus any cloud in-basket you wire in via an extension). Slower, more deliberate. See [Weekly triage](#weekly-triage) below.

### Routing when the user runs bare `/triage` (no mode argument)

1. If `enable_triage_habits=false`, default to daily without reading `habits.csv`. Explicit `/triage weekly` still runs the complete weekly workflow.
2. Otherwise, look up the row whose `system_key=brain.triage.weekly` in `<brain>/tasks/habits.csv`. Compare its `due_date` and `status` to today.
3. **If weekly triage is due today or past-due** AND `status != done`, ASK the user once:
   > "Weekly triage is due (since [date]). Run weekly or daily?"
   Then run whichever they pick.
4. **Otherwise** (weekly habit is `done` for this cycle, or its `due_date` is still in the future), **default to daily, no confirmation.** Just start the daily workflow.

Do NOT ask the user "which mode?" outside of case (2). Saving their time is the priority.

<a id="skip-daily-triage-today--skip-the-morning-triage-habit-run-nothing-else"></a>

### "Skip daily triage today" - skip the Morning Triage habit, run nothing else

If the user says we can **skip** daily triage for the day ("skip daily
triage", "no triage today", "we can skip triage"), run nothing else. If
managed triage habits are enabled, complete the protected daily occurrence
with `brain habits complete-managed-triage daily`. If they are disabled,
acknowledge the skip
without reading or mutating `habits.csv`. In either case, send the optional
background completion signal as the final action when its two environment
variables are present.

---

# Daily triage

The original "morning triage": a fast pass over past-due tasks. The user can
have 50+ past-due tasks, and every question round is a chance for them to walk
away while the day's plan waits. So daily triage **proposes, it does not ask.**
It gathers every candidate, decides a default for each one, shows the whole
plan once, and takes one round of answers. Then it applies the plan and
finishes.

## When to invoke

- User says: "triage my tasks", "run triage", "morning triage", "let's clean up past-due", "what's slipping?", "what's overdue?", "what's about to slip?", "what's at risk?", "what's rotting?", "what's been ignored?", "any deadwood?".
- `/triage daily` or `/triage morning` invoked directly.
- Bare `/triage` when the weekly habit isn't due (see [Routing](#routing-when-the-user-runs-bare-triage-no-mode-argument)).
- During /agenda planning when there are 10+ past-due tasks (defer to /triage rather than handling inline).

## Daily question budget

Daily triage asks **at most one question round** per run, and that round is
[Step 3](#step-3--ask-the-one-round). Everything else is silent: gathering,
scanning, planning, applying, and the summary. The round holds every decision
the run needs:

1. The plan confirmation (always, when the plan changes anything).
2. Hard-deadline review (only when [Step 5](#step-5--hard-deadline-review) has rows).
3. At most one question contributed by an extension step.

Steps delegated after the round ask nothing. An /agenda build that triage hands
off takes the round's answers as settled (see
[Triage handoff](../agenda/references/planning.md#triage-handoff)).

Extension steps that need a decision contribute it to this round. They never
open their own round, and they never re-ask about a task this run has already
decided. An extension step that runs after the round (a merge step, for
example) reports what it found instead of asking. The only extra rounds are
ones the user explicitly asks for, such as a one-by-one walk of a named group.
When the round would exceed the question surface's capacity, merge the
lowest-value questions into the plan confirmation's free-form answer. Every
question offers a free-form answer so the user can name exceptions in one line
("T41 today, T18 drop, walk the finance group one by one").

When the plan is empty and nothing else needs a decision, the run asks
zero questions. Never add a second round to clarify an answer. Apply the clear
parts, keep the rest at the plan default, and say what you assumed in the
summary.

<!-- brain:ext triage:daily-open -->

<a id="parallel-sub-agents--launch-now-before-the-task-passes"></a>

## Parallel sub-agents - launch now, before the task passes

Daily triage can run **background sub-agents in parallel with the rest of daily
triage** - independent jobs (an inbox sweep, an issue-tracker reconcile, a report
build) that don't depend on the planning passes and would only burn wall-clock
if run serially first. An extension registers each such sub-agent at the hook
below.

**Launch every registered sub-agent now, in the background, and do NOT block on
them here.** Record each sub-agent's handle so you can collect it when the task
passes are done. The task-triage passes (Steps 0–8) run *while* the sub-agents
work. Every registered sub-agent must also declare, at `triage:daily-merge`
(collected just before Step 9), how its output folds back into the run's output
before the run finalizes - launching a sub-agent here without a matching merge
rule there is a mistake. If no sub-agent is registered, this is a no-op - go
straight to Step 0.

<!-- brain:ext triage:daily-subagents -->

<a id="step-0--backlog-auto-purge-silent-runs-every-triage"></a>

## Step 0 - Backlog auto-purge (silent, runs every triage)

Before the task-triage steps, run the 6-month backlog purge:

```
brain backlog purge
```

It deletes any `status=backlog` task whose `backlogged_date` is >6 months
old, leaves a breadcrumb in the project metadata for any deleted
project-linked task, and prints nothing. **This is deliberately silent:
never warn before deletion, never report which tasks were deleted, never
mention the purge ran.** Just run it and move on. (See /todo SKILL.md
"Backlog".)

<!-- brain:ext triage:daily-linear -->

<a id="step-1--pick-the-most-useful-grouping"></a>

## Step 1 - Gather the candidates (silent)

Read `tasks.csv` with a real CSV parser. Never read `habits.csv` here (see
[Triage never touches habits](#triage-never-touches-habits-hard-invariant)).
Collect, without asking anything:

- **Past-due rows:** `status` not `done`/`backlog` and `due_date < today`.
- **Today's existing load:** open rows already due today, with their
  `estimated_duration`.
- **Load for each of the next five days:** the same sum per day.
- **At-risk hits** from [Step 6](#step-6--forward-looking-at-risk-scan) and
  **stale waits** from its stale-waiting check.
- **Chronic-ignore hits** from [Step 7](#step-7--chronic-ignore-sweep).

Compute dates in the shell, never in your head. Treat an empty
`estimated_duration` as 30 minutes.

<a id="step-2--present-the-groups"></a>
<a id="step-2--build-the-plan"></a>

## Step 2 - Build the plan

Give every past-due and chronic-ignore row exactly one proposed action. Use
these defaults, in this order:

1. **Hard deadline.** A `hard_deadline=true` row is never bulk-deferred.
   Exclude every `hard_deadline=true` row from every proposed defer. Propose
   **Today** for it, and send it through the
   [hard-deadline review](#step-5--hard-deadline-review) when its
   `defer_count >= 1`. Plan consent does not replace that individual decision.
2. **Chronically deferred.** `defer_count >= 5`, no hard deadline, not `p0`,
   not MIT: propose **Move to backlog** (`brain backlog park <T###>`). The
   user can restore it later. Another defer is the wrong default at this point.
3. **Chronic-ignore hit** (`brain tasks chronic`, see Step 7): propose **Move to
   backlog** for the same reason. Drop stays available as a free-form exception.
4. **Today.** A `p0` or MIT row stays for today while it fits today's
   remaining capacity. Estimate that capacity the way /agenda does: live
   clock, explicit anchors, and calendar busy blocks (see
   [capacity](../agenda/references/planning.md#capacity-and-placement)),
   minus the work already due today. Fill in priority order and stop when
   the time runs out. Never propose more for today than fits.
5. **Spread.** Every other row moves to one of the **next five days**. Take
   rows in priority order, then oldest due date first. Put each one on the day
   with the smallest load (existing work due that day plus rows already placed
   there), and break ties toward the earliest day. Skip a day only when an
   explicit anchor says it is unavailable. Spreading keeps a wall of +7d
   defers from piling up on a single day.

Native rules still apply when the plan runs. A `status=waiting` row or a row
with a non-empty `blocked_by` defers with no `defer_count` penalty. Rows in
the **Today** bucket move with `brain tasks set <T###> --due <today>`, which
keeps their MIT and priority. Spread rows move with
`brain tasks defer <T###> <YYYY-MM-DD>`.

A task with `estimated_duration > 90` or scope verbs (`launch`, `build`,
`migrate`, `research`) that keeps slipping can carry a note in the plan
suggesting `/todo turn-into-project`. It is a note, not a question.

<a id="step-3--bulk-operations-per-group"></a>
<a id="step-3--ask-the-one-round"></a>

## Step 3 - Ask the one round

Show the plan as one compact block, grouped by action. Put the deadline first
on every row (see [question shape](#asking-the-user-for-per-task-actions-use-askuserquestion)):

```
Daily triage plan (14 past-due, 1 chronic):

  Today (fits 1h40m):  T41 Send the vendor contract [p0, MIT]
  Wed 7/9:             T12, T15, T33
  Thu 7/10:            T19, T27
  Fri 7/11:            T22, T30, T35 ...
  Backlog (def >= 5):  T18 (def=8), T9 (def=6), T52 (chronic, stale_21d)

At-risk (FYI, no action): T60 due 7/12 (def=2), T61 due 7/14 (p1, not started)
```

Then ask the single round from the
[question budget](#daily-question-budget) on the available question surface.
The plan confirmation looks like this:

```
header:   Plan
question: Apply this triage plan? (15 tasks: 1 today, 11 spread over 7/9-7/13, 3 to backlog). Name exceptions in free text, e.g. "T12 today, T18 drop".
options:  Apply (Recommended) / Apply, nothing today / Leave everything as is
```

Hard-deadline review rows go in one more question of the same round (see
Step 5). Do not ask about at-risk rows, stale waits, or anything the plan
already decided.

<a id="step-4--1-by-1-walk"></a>

## Step 4 - Apply the plan

Apply the confirmed plan with the user's exceptions folded in. Run every
mutation without further prompts. A free-form answer can ask to walk a group
one task at a time. Honor it, but batch that walk into as few rounds as the
question surface allows.

- **Drop** (only as an explicit exception): preview all named IDs in one
  command with `brain tasks remove T17 T18 --dry-run --json`. Apply with
  `--yes` and the preview fingerprint as `--expect-plan` when the user's
  answer already named those IDs and no dependency or linked-issue policy
  choice is open. If a policy choice is open, keep the row at its plan default
  and report the choice in the summary instead of opening a new round. Follow
  [/todo's removal contract](../todo/SKILL.md#remove-tasks); never apply
  removal to a habit (see
  [Triage never touches habits](#triage-never-touches-habits-hard-invariant)).
  Submit all consented IDs in one command.
- **Backlog projects:** when a backlogged task belongs to a project, report
  the project follow-up from
  [/todo "Backlog ↔ projects"](../todo/SKILL.md) in the summary. Do not ask.
- After applying, flag every row whose `defer_count >= 3` in the summary.

<a id="step-5--hard-deadline-review"></a>

## Step 5 - Hard-deadline review

Before any proposed defer, screen every `hard_deadline=true` row:

- Hard-deadline rows never join a spread or any other bulk defer.
- A hard-deadline row whose `defer_count >= 1` gets a deadline check in the
  [one round](#step-3--ask-the-one-round). Ask one multi-select question:
  > "These hard deadlines have already slipped. Check any that are no longer real: T### '<name>' (due <date>, def=N), …"
- Unchecked rows keep their hard deadline and stay in **Today**. Checked rows
  lose the `hard_deadline` flag and join the spread. A free-form answer can
  set a new date for a specific row instead.

<a id="step-6--forward-looking-at-risk-scan"></a>

## Step 6 - Forward-looking at-risk scan

This scan never asks a question. It runs every daily triage, even when
past-due was empty, and its hits appear as an **FYI line** in the plan and the
summary. At-risk tasks aren't late yet, and "leave as is" was almost always
the answer. The user can still act on any of them in the free-form answer.

**Deadline window (explicit user rule):** only surface an at-risk task when its `due_date` is **within 8 days** (`<= today + 8`). Flag-when-close, not early-warning: a task in danger but still 9+ days out is left alone until it gets closer. (The chronic-ignore sweep in Step 7 uses a tighter 3-day window.)

### Stale-waiting check

A task in `status=waiting` is paused on an **external** party, so its slipping isn't avoidance. But waiting forever is its own failure mode. Run:

```
brain tasks stale-waiting --pretty
```

List every task that has been waiting **more than 7 days** (`waiting_since`) in the FYI line, with the party to follow up with when you can infer it from `task_name`/`see_also`. A `status=waiting` row with an empty `waiting_since` is listed too. Do not ask whether to create a check-in task; the user can request one.

### At-risk criteria

Filter `tasks.csv` for rows where `status != done`, `start_date` is empty or `<= today+8`, and **any** of the following match. Every criterion is gated by the 8-day window above:

1. **Hard deadline, never started** - `hard_deadline=true` AND `due_date <= today+8` AND `status == not_started`.
2. **Hard deadline already pushed** - `hard_deadline=true` AND `defer_count >= 1` AND `due_date <= today+8`.
3. **Repeat-deferral pattern** - `defer_count >= 2` AND `due_date <= today+8`.
4. **High-priority not started** - `priority in (p0, p1)` AND `status == not_started` AND `due_date <= today+8`.
5. **Long task, no progress, due soon** - `estimated_duration >= 60` AND `status == not_started` AND `due_date <= today+8`.

Skip rows the plan already covers. Show at most five hits, most urgent first,
then "+N more".

<a id="step-7--chronic-ignore-sweep"></a>

## Step 7 - Chronic-ignore sweep

Find tasks that have rotted: not past-due (that's the plan's main set) and not about to slip (Step 6), but **inert**. These are deadwood candidates, and they feed the plan with a **Move to backlog** default ([Step 2](#step-2--build-the-plan)). This sweep asks nothing on its own.

**Tasks only.** An inert habit is not deadwood - a habit nobody has done in weeks is still a habit the user keeps, and its pending row is the only record of the cadence. The detector reads `tasks.csv`; never extend this sweep to `habits.csv` (see [Triage never touches habits](#triage-never-touches-habits-hard-invariant)).

### Eligibility (computed by the detector)

A task in `tasks.csv` qualifies if `status != done`, its deadline is imminent or absent (`due_date` empty OR `today <= due_date <= today + 3d`), and at least one of:

**Never nag about far-off deadlines (explicit user rule).** A dated task is surfaced only once it's within **3 days** of its `due_date`. Anything further out is left alone. Undated thin rows have no deadline to be "far from", so they stay eligible. Past-due rows (`due_date < today`) belong to the plan's past-due set, not this sweep.

1. **`stale_21d`** - `today - last_touched >= 21d`. Primary signal.
2. **`stuck_in_progress`** - `status == in_progress` AND `today - last_touched >= 14d`.
3. **`captured_forgotten`** - `today - created_date >= 60d` AND `status == not_started` AND empty `notes`, empty `estimated_duration`, empty `project`.

Don't apply these filters in your head - LLMs are bad at calendar math. Run the script:

```
brain tasks chronic
```

Outputs one JSON object per matching task (sorted by max-days-since-touch first) with `task_id`, `task_name`, `reasons[]`, `days_since_touch`, `days_since_create`, `status`, `priority`, `task_type`, `due_date`, `defer_count`, `project`, `hard_deadline`. Pipe to `--count` for a quick number, `--pretty` for human-readable.

Free-form exceptions for a chronic row: **revive** (`brain tasks touch <T###>`, another 21 days before it reappears), **start now** (`status=in_progress`, `start_date=today`), **defer to date**, or **drop** (Step 4's removal rules).

<a id="step-8--summary"></a>

## Step 8 - Summary

After triage:

```
Triage complete (1 question round):
  - 11 tasks spread over 7/9-7/13
  - 1 kept for today
  - 3 moved to backlog
  - 0 dropped
  - High-defer (>= 3): T22 (4), T30 (3)

  Remaining past-due: 0
  At-risk (next 8d, FYI): T60, T61
  Chronic-ignore: 1 (T52 to backlog)
```

Omit lines that are zero, except "Remaining past-due". List any assumption you
made about an unclear answer. Any integration passes you run from an
extension (email, issue-tracker reconcile) add their own summary lines above
these.

## Collect sub-agents, then merge their output (before you finalize anything)

If you launched any sub-agents at `triage:daily-subagents`, the run is **not**
finished until they all complete and their output has been merged in. Do this
before Step 9:

1. **Wait for every registered sub-agent to finish** and collect each one's
   result (its output markdown / artifacts). Never finalize, render, or hand the
   user any combined output of the run while a sub-agent is still running - a
   printable the run produces must contain their work. This is the hard rule.
2. **Merge each finished sub-agent's output into the run's output**, following
   the merge instructions that sub-agent's extension declares at the hook below.
   If a sub-agent failed or was skipped, say so in the Step 8 summary and proceed
   with what you have.
3. Only once all sub-agents are collected and merged do you produce or regenerate
   the run's final output(s), then move to Step 9.

Core itself produces no such output - this section only guarantees that *if* an
extension does, the sub-agents' work is folded in first. With no registered
sub-agents, this step is a no-op.

<!-- brain:ext triage:daily-merge -->

If the user requested a new/revised plan, or a contributed step requires one,
delegate that work to [/agenda](../agenda/SKILL.md) before completion. Core triage
does not require an agenda, PDF, or any other artifact. Ordinary task mutations
already synchronize existing plans through Rust.

<a id="step-9--mark-morning-triage-habit-done"></a>

## Step 9 - Mark Morning Triage habit done

After the daily triage process completes (user has either resolved every past-due task or explicitly skipped remaining groups):

1. If managed triage habits are enabled, complete the protected daily occurrence via:
   ```
   brain habits complete-managed-triage daily
   ```
   The helper resolves `system_key=brain.triage.daily`, records completion,
   and advances the recurring chain. Do not use the ordinary task or habit
   completion commands for a managed row.
2. If managed triage habits are disabled, skip all habit lookup and mutation.
   The manual triage pass is still complete.
   Native managed completion already synchronizes any existing agenda and PDF.
   Disabled completion changes no habit or agenda. Do not manually rewrite or
   redundantly refresh either artifact. The startup check uses the stable
   `system_key=brain.triage.daily`; visible names and occurrence IDs can change.

3. **If this pass is running as a background skill-session tab, signal completion.**
   When the tasks view launches daily triage in its own ephemeral tab it sets two
   environment variables in this session: `BRAIN_SESSION_DONE_URL` (a local
   brain-server URL) and `BRAIN_SESSION_TOKEN` (a one-time token), and the prompt
   you were launched with already spells out the POST. **Only when both variables
   are set**, after managed completion (or its disabled no-op), send that completion signal so the
   tasks view auto-closes this tab.

   **The signal carries a `require` list - the paths of every output this run
   committed to producing that must exist before the tab may close.** Core
   itself declares none, so by default the list is empty. But this run may have
   had extension steps rendered into it (at the hooks above, and at the
   required-outputs hook just below) that produce a printable, a report, a
   digest, or similar. If any such step told you it produces an output artifact
   that must be on disk before closing, put that artifact's path in `require`.
   brain holds the signal open and will **not** close the tab until every listed
   path exists - so a premature signal can't kill the session before the run's
   declared outputs land. With an empty list the tab closes as soon as the
   signal arrives.

   Declare here any required output paths (an extension fills this in; if none
   is rendered, there are none and `require` stays empty):

   <!-- brain:ext triage:daily-required-outputs -->

   Then POST, substituting the required-output paths as a JSON string array (use
   `[]` when there are none):
   ```
   [ -n "$BRAIN_SESSION_DONE_URL" ] && [ -n "$BRAIN_SESSION_TOKEN" ] && curl -fsS -X POST "$BRAIN_SESSION_DONE_URL" \
     -H 'Content-Type: application/json' \
     -d "{\"token\": \"$BRAIN_SESSION_TOKEN\", \"require\": [<required-output paths, or empty>]}" \
     >/dev/null
   ```
   Do this as the very last action of the pass: once every required output
   exists the tab closes as soon as the signal lands, so anything you still need
   to show the user must come first. When the variables are unset (a normal
   in-session `/triage`, not a background tab), skip this step entirely; there is
   nothing to close.
   Report a failed signal and retain the session for recovery.
   Always send the background completion signal whether managed habits are enabled or disabled.

**"Skip daily triage today" means the day is handled.** Do not run Steps
0-8. Follow the feature-gated skip path above, then send the optional
background completion signal. With managed habits enabled, the protected
helper records the decision and advances the chain. With them disabled,
there is deliberately no habit state to update.

---

# Weekly triage

A weekly in-basket review. This is **not** just task management - it's the user's second-brain in-basket sweep. Core weekly triage processes **two local in-baskets**: the scratch notepad (`<brain>/scratch.md`) and the `capture/` directory, the user-managed in-basket where they dump notes, photos, PDFs, and anything else during the week. Wire any cloud in-basket you use in through the `triage:weekly-inboxes` extension point below.

**End state: every in-basket is EMPTY.** Every item gets routed to a real home (task, note, project, or a "decide what to do with this" follow-up task).

Default to making decisions yourself (this is assistant mode). Only ask for confirmation when you are genuinely unsure of intent.

## When to invoke

- `/triage weekly` invoked directly.
- Bare `/triage` when the Weekly in-basket processing habit is due or past-due (see [Routing](#routing-when-the-user-runs-bare-triage-no-mode-argument)) AND the user picks "weekly".
- User says: "weekly triage", "in-basket processing", "process my scratch", "clear the in-basket", "empty my in-basket", "weekly review".

<a id="step-1--process-the-local-in-baskets-scratchmd--capture"></a>

## Step 1 - Process the local in-baskets (`scratch.md` + `capture/`)

Two local in-baskets, one pass. Aim to empty both, but never discard a failed or
unreadable input to claim success. Routing is the same for
both (classify, then send each item to a task, a note, or a clarifying
question), so the shared routing rules are stated once in 1a and referenced
from 1b.

<a id="1a--the-scratch-notepad-brainscratchmd"></a>

### 1a - The scratch notepad (`<brain>/scratch.md`)

1. **Read** `<brain>/scratch.md` in full (the local scratch notepad - anything the user dumped here during the week).
2. **Walk it top to bottom**, splitting into discrete items at natural boundaries (paragraph breaks, blank lines, bullet groups, headings). Treat each coherent chunk as one item.
3. **For each item, classify** as TASK, NOTE, or UNSURE - see [Classification rules](#classification-rules).
4. **Process** (these three routes are the shared routing rules 1b reuses):
   - **TASK** → invoke `/todo` to create the task with the best-guess fields (priority, due_date, task_type, duration). If the item is clearly a project (multi-step, scope verbs like `launch`/`build`/`migrate`/`research`, multiple checkboxes), CONFIRM with the user and run `/todo turn-into-project`. When creating sub-tasks of a converted project, set `blocked_by` relationships explicitly per [task-project-link.md](../projects/references/task-project-link.md).
   - **NOTE** → place it in the appropriate `/organize` location (project / area / resource). Use the `organize` skill's placement rules. Create a new subdirectory only if no existing home fits, and ask for confirmation only when you're genuinely unsure. Pair PDFs/media with their notes per the brain conventions.
   - **UNSURE** → ask the user a single specific question: "Found '<short paraphrase>' - looks like it could be X or Y. What did you mean?" Move on to the next item while waiting if the user is batching answers.
5. **Remove the item from scratch.md** only after its destination/task is verified. Edit `<brain>/scratch.md` as you go.
6. At the end of 1a, `<brain>/scratch.md` should be **empty** (or contain only items the user explicitly chose to leave - but see [Deferring ≠ leaving in the inbox](#deferring--leaving-in-the-inbox)).

<a id="1b--the-capture-in-basket"></a>

### 1b - The `capture/` in-basket

If `capture/` contains inputs, delegate this pass to
[/process-capture](../process-capture/SKILL.md#process). It owns inventory,
inspection, idea grouping, placement, task creation, and input-to-result accounting.
Require its report of completed and blocked inputs before claiming the inbox is
empty. Keep unreadable/failed material in place and report the blocker. An empty
capture directory needs no delegation. Plain capture processing does not complete
the weekly habit; that belongs to this full weekly workflow.

<!-- brain:ext triage:weekly-inboxes -->

<!-- brain:ext triage:weekly-linear -->

<a id="step-2--monthly-check--backlog-review"></a>

## Step 2 - Monthly check + backlog review

Every weekly triage also checks whether it's the **monthly** triage.
"Monthly" is not its own command - it's simply the **first weekly triage
of a calendar month**, and its only extra job is reviewing the backlog.

1. **Detect:** `brain triage state`.
   If `is_monthly` is `false`, skip this step entirely. If `true`, do the
   dedupe + backlog review below, then mark it: `brain triage state
   --mark` (so the next weekly triage this month is just weekly).
2. **Dedupe backlog vs active (monthly only, silent):**
   `brain backlog dedupe`. This deletes
   any backlog task that has an active-list twin which was *created after*
   the task was backlogged - i.e. the user already re-created (revived) it
   by hand, so the backlog copy is a stale duplicate. It does nothing but
   delete the duplicate backlog row, and prints nothing. **Silent like the
   purge: don't announce what (if anything) was deduped.** Run it before
   the backlog review so resurfaced candidates are dup-free.
3. **Backlog review (monthly only):** `brain backlog`.
   The goal is **resurfacing**, not clearing: surface only backlog items
   that (a) look **relevant to current work** and (b) look **doable given
   current time/demands**. This is how the user rediscovers tasks they
   parked when they were irrelevant or there was no time - that might
   matter now. **Do NOT walk every backlog item** - that defeats the
   purpose and wastes the user's time. Pick the handful that genuinely
   merit a second look and ask, via the available question surface, whether to
   **leave it parked** or **restore** each (`brain backlog restore <T###>`,
   then set a fresh `due_date`/`priority`). **`Leave parked`
   is ALWAYS the first option** - the backlog default is to stay parked,
   and surfacing an item here is a preview, not a nudge to restore it
   (the same reason daily at-risk hits are FYI only). Items that aren't
   obviously relevant-and-doable stay in the backlog silently; the
   6-month purge (Step 0) eventually clears the truly dead ones with no
   prompt.

<a id="step-3--mark-the-weekly-habit-done"></a>

## Step 3 - Mark the Weekly habit done

After **all** in-baskets are empty:

1. If managed triage habits are enabled, complete the protected weekly occurrence via:
   ```
   brain habits complete-managed-triage weekly
   ```
   The helper resolves `system_key=brain.triage.weekly`; visible names are not identity.
2. If managed triage habits are disabled, skip habit lookup and mutation. The
   weekly workflow remains complete once its in-baskets are empty.
   Native completion syncs an existing agenda/PDF; a disabled helper does nothing.
   Do not rewrite agenda artifacts or perform a second synchronization.

<a id="step-4--offer-to-chain-into-daily-triage"></a>

## Step 4 - Offer to chain into daily triage

Once weekly processing is complete and managed completion is handled (or disabled), ask once:

> "Weekly triage complete - the in-baskets are empty. Want to run daily triage now?"

- If yes → start the [Daily triage](#daily-triage) workflow from its opening hooks, sub-agent phase, and Step 0, then all task passes.
- If no → end here.

---

# Shared rules

## Never auto-mutate

Apart from the explicit maintenance exceptions in daily Step 0 and the monthly
check, triage never deletes, defers, re-prioritizes, or routes anything without
an explicit user choice. In daily mode, confirming the plan is that choice for
the eligible selected rows the plan lists, with the user's exceptions applied.
Plan consent and bulk options never waive individual hard-deadline review or
include protected rows in a bulk defer. "Leave everything as is" changes
nothing. In weekly mode, your best-guess routing of a TASK or NOTE counts as a decision the user has implicitly authorized - but anything UNSURE must be confirmed.

<a id="asking-the-user-for-per-task-actions-use-askuserquestion"></a>

## Asking the user for per-task actions

Use the available frontend question surface for concrete action choices, including
closing offers. When none is available, ask a concise textual question with the
same choices. Batch within that surface's supported capacity. Keep the
question self-contained: deadline, ID, name, reason, options, and recommendation
belong together. Never depend on a particular frontend tool name or four-question
limit. Preserve an open/freeform response when the predefined choices do not fit.
Daily triage asks only inside its [one round](#daily-question-budget); these
rules shape that round and the weekly questions.

**Lead with the deadline, before the task ID - always.** Every plan row and
every question that names a task starts with its due date in **bold**, because
knowing instantly whether a decision is about something due *today* vs *weeks
out* is the single most important cue for fast triage. Format:

- Due today → `**Due TODAY:** T### '<name>' [p0/work, def=1] …`
- Future → `**Due 7/15:** T17 '<name>' [p2/personal, def=3] …`
- Past-due → `**Due 7/5 (2d late):** T### …`
- No due date → `**No due date:** T### …`

After the bolded deadline, give the task ID, name, type/priority, and key flags
(`defer_count`, `hard_deadline`). In a compact plan block, a row can drop the
name when the ID and deadline are enough to recognize it.

**`Move to backlog` is a standing action everywhere.** It parks the task
(clears its dates, hides it from active views) via `brain backlog park <T###>`,
which stops the per-triage nagging without losing the task. Daily triage
proposes it by default at `defer_count >= 5` ([Step 2](#step-2--build-the-plan)).
In weekly mode, surface it as an explicit option whenever `defer_count >= 4`.

**Open questions** can use concise prose; concrete action menus use the available
surface or its textual fallback.

- **Hard-deadline confirmations** (daily Step 5) - one multi-select inside the daily round. They're not "pick an action," they're "is this still real?"
- **In-basket UNSURE clarifications** (weekly Step 1) - "I found '<x>', did you mean A or B?" is an open intent question, not a fixed action menu; prose is fine. (If it does collapse to a clean A/B/C action choice, use the tool.)

## Classification rules

- **TASK** - imperative phrasing ("Send X to Y", "Email so-and-so", "Fix the Z bug"), TODO-list bullets, named actions, anything that requires the user to *do* something.
- **NOTE** - reference content, links, quotes, recipes, code patterns to remember, half-formed ideas, design sketches. Static knowledge, not action.
- **UNSURE** - terse fragments, opaque code blocks, single words, weird syntactic things, anything that could plausibly be either. ASK the user. Don't guess.

This is an in-basket of the user's random thoughts captured at random times. Some of it won't make sense. **It is fine to be unsure** - what's NOT fine is asking for confirmation when you're already sure. Asking too much wastes the user's time.

In `capture/` the same three classes apply to **non-text items too**. A photo of a whiteboard is usually a NOTE; a photo of a receipt may be a TASK ("file the expense"); a screenshot of an error may be either. Classify on what the item *is about*, after looking at it, never on its file extension.

## Deferring ≠ leaving in the inbox

If the user can't decide what to do with a note or task right now, **the inbox is still not where it stays.** This holds for every in-basket, `capture/` included: parking an undecided item back in `capture/` is the one thing `capture/` is not for. Deferring means creating a TASK in `tasks.csv` whose name is the deferred decision itself - e.g.:

> `T###` "Decide what to do with the 'multi-language querying' demo idea - see scratch capture from 2026-06-10"

…and removing the original item from the inbox. The inbox empties out; the decision moves into the task system where it can be triaged like any other open loop.

For a `capture/` item the same move also needs a home for the *material*: file it in the PARA bucket that fits best, then point the task at it (`see_also`), so the decision task carries the thing it is about.

## Reference

See [references/heuristics.md](references/heuristics.md) for the retained compatibility path to this canonical daily procedure. The task ↔ project linkage rules used during weekly TASK routing live in [canonical task-project linkage](../projects/references/task-project-link.md).
