---
name: triage
description: Use when the user asks to triage tasks, run "morning triage", run weekly review / in-basket processing, clean up past-due tasks, or asks "what's slipping?" / "what's about to slip?" / "what's rotting?" — supports daily (past-due cleanup + at-risk preview + chronic-ignore sweep) and weekly (in-basket processing + monthly backlog review) modes.
---

# triage

Throughout this skill, `<brain>` is your brain root — the directory `brain
config get root` returns (default `~/brain`) — and `~/.agents/skills/todo/scripts/`
is where `brain skills sync` installs the `/todo` skill's helper scripts. Both
resolve without you hardcoding a personal path.

## Establish "today" from the system clock FIRST (before anything else)

Every triage action is date-sensitive. **Before doing anything, run
`date +%F` and treat its output as the authoritative "today."** Do NOT
trust the `currentDate` / "Today's date is …" value from your session's
opening context — it is frozen at session start, and this transcript can
stay open for **days** (the user may not close the session overnight). A
stale cached date silently marks the **wrong day's Morning Triage habit**
done (e.g. a "skip triage" honored against yesterday's occurrence while
today's stays pending) and mislabels everything downstream. Re-run
`date +%F` at the start of the routing check, the skip flow, and Step 9,
and resolve the habit row by *that* date. Shell clock wins over context,
always. (Mirror of /todo SKILL.md operating principle 5.)

## Personal-assistant mode

This is a personal/executive assistant skill. Load who you're assisting:
run `brain personalize show` and honor their `role`/`works_for` (both may be
unset — then keep the framing neutral). **Top priority is saving the user's
time.** Group first, walk individuals only when grouping doesn't apply. Be
blunt. Make obvious decisions. Ask the user only when you genuinely can't
proceed — and when you ask, batch questions and keep them short.

Sibling skills: /todo (tasks + agenda), /second-brain (knowledge management). All engage the same assistant mode.

## Modes

This skill has two distinct workflows:

- **`/triage daily`** (alias: `/triage morning`) — past-due task triage. The fast-paced "morning triage" pass over `tasks.csv`. See [Daily triage](#daily-triage) below.
- **`/triage weekly`** — in-basket processing across your brain's scratch inbox (and any cloud in-basket you wire in via an extension). Slower, more deliberate. See [Weekly triage](#weekly-triage) below.

### Routing when the user runs bare `/triage` (no mode argument)

1. **Look up the Weekly in-basket processing habit** in `<brain>/tasks/habits.csv` (the row whose `task_name` contains "Weekly in-basket processing"). Compare its `due_date` and `status` to today.
2. **If weekly triage is due today or past-due** AND `status != done` → ASK the user once:
   > "Weekly triage is due (since [date]). Run weekly or daily?"
   Then run whichever they pick.
3. **Otherwise** (weekly habit is `done` for this cycle, or its `due_date` is still in the future) → **default to daily, no confirmation.** Just start the daily workflow.

Do NOT ask the user "which mode?" outside of case (2). Saving their time is the priority.

### "Skip daily triage today" — skip the Morning Triage habit, run nothing else

If the user says we can **skip** daily triage for the day ("skip daily
triage", "no triage today", "we can skip triage"), do **not** just drop
it — **skip today's Morning Triage habit** and run nothing else. Morning
Triage is a **daily** habit, so this is just the general
[Skipping a habit](../todo/SKILL.md#skipping-a-habit) rule: run
`brain habits skip "Morning Triage"`,
which for a daily habit marks today's occurrence `done`. There is no
daily-triage-specific skip path anymore — it's the same deterministic
script every habit skip uses. Skipping is an explicit decision that the day
is handled, so the habit must reflect that: it's what the brain tasks view
checks to stop nagging (`daily_triage_name_pattern` → `check_daily_triage`)
and what keeps the agenda's habit state honest. Full rationale in
[Step 9](#step-9--mark-morning-triage-habit-done). This holds even when
the skip is mentioned in passing during another flow (agenda build,
weekly triage) — skip it the moment they say so.

---

# Daily triage

The original "morning triage" — a fast pass over past-due tasks that bulk-groups them for one-shot decisions. The user can have 50+ past-due tasks; going 1-by-1 is unworkable. Always group first; offer bulk operations before walking items individually.

## When to invoke

- User says: "triage my tasks", "run triage", "morning triage", "let's clean up past-due", "what's slipping?", "what's overdue?", "what's about to slip?", "what's at risk?", "what's rotting?", "what's been ignored?", "any deadwood?".
- `/triage daily` or `/triage morning` invoked directly.
- Bare `/triage` when the weekly habit isn't due (see [Routing](#routing-when-the-user-runs-bare-triage-no-mode-argument)).
- At the top of /todo's agenda flow when there are 10+ past-due tasks (defer to /triage rather than handling inline).

<!-- brain:ext triage:daily-open -->

## Parallel sub-agents — launch now, before the task passes

Daily triage can run **background sub-agents in parallel with the rest of daily
triage** — independent jobs (an inbox sweep, an issue-tracker reconcile, a report
build) that don't depend on the grouping / at-risk / chronic passes and would only
burn wall-clock if run serially first. An extension registers each such sub-agent
at the hook below.

**Launch every registered sub-agent now, in the background, and do NOT block on
them here.** Record each sub-agent's handle so you can collect it when the task
passes are done. The task-triage passes (Steps 0–8) run *while* the sub-agents
work. Every registered sub-agent must also declare, at `triage:daily-merge`
(collected just before Step 9), how its output folds back into the run's output
before the run finalizes — launching a sub-agent here without a matching merge
rule there is a mistake. If no sub-agent is registered, this is a no-op — go
straight to Step 0.

<!-- brain:ext triage:daily-subagents -->

## Step 0 — Backlog auto-purge (silent, runs every triage)

Before the task-triage steps, run the 6-month backlog purge:

```
python3 ~/.agents/skills/todo/scripts/purge_old_backlog.py
```

It deletes any `status=backlog` task whose `backlogged_date` is >6 months
old, leaves a breadcrumb in the project metadata for any deleted
project-linked task, and prints nothing. **This is deliberately silent:
never warn before deletion, never report which tasks were deleted, never
mention the purge ran.** Just run it and move on. (See /todo SKILL.md
"Backlog".)

<!-- brain:ext triage:daily-linear -->

## Step 1 — Pick the most useful grouping

Try these in order; commit to the **first** one whose largest bucket has at least 3 items (otherwise you're just walking 1-by-1 anyway):

1. **By `task_type`** — "8 code tasks past due", "12 personal tasks past due". This is usually the most natural grouping.
2. **By overdue bucket** — `<7d`, `7-30d`, `>30d`. Surfaces the "ancient zombies" (>30d) for a separate question.
3. **By `priority`** — useful when type-grouping is balanced but priority is skewed.
4. **By `project`** — when many past-due tasks share a project.

If multiple groupings tie, prefer the one that gives the smallest number of groups (less cognitive load for the user).

## Step 2 — Present the groups

One line per group:

```
Past-due triage (47 tasks):

  personal  (18) — defer all 7d / drop all / 1-by-1 / skip
  code      (12) — defer all 7d / drop all / 1-by-1 / skip
  work      (8)  — defer all 7d / drop all / 1-by-1 / skip
  finance   (4)  — defer all 7d / drop all / 1-by-1 / skip
  errands   (3)  — defer all 7d / drop all / 1-by-1 / skip
  > 30d old (12) — defer all 7d / drop all / 1-by-1 / skip
```

The "> 30d old" cross-cut is shown even when the primary grouping is by `task_type` — it's the highest-signal group.

## Step 3 — Bulk operations per group

- **Defer all N days** — accept any positive integer; quick-picks are `1`, `7`, `14`. Increments `defer_count` for each **except no-penalty defers**: a task with `status=waiting` or a non-empty `blocked_by` defers without raising `defer_count` (the slip isn't the user's fault). `defer_task.py` handles this automatically; `--no-count` forces it for other not-our-fault cases. If any task in the group hits `defer_count >= 3` *after* the bulk defer, flag it in the summary.
- **Drop all** — confirms with the count, then removes them from the CSV. Never silently destructive.
- **1-by-1** — walks the group with per-task prompts (see Step 4).
- **Skip group** — moves to next.

## Step 4 — 1-by-1 walk

For each task in the chosen group, ask via **`AskUserQuestion`** (see [Asking the user for per-task actions](#asking-the-user-for-per-task-actions-use-askuserquestion)). Batch up to 4 tasks per call.

Example question payload (one task):

```
header:   T108
question: **Due 7/5 (1d late):** T108 "Check in with the vendor on the billing-side bug" [p2/work, def=0]. What's the call?
options:  Done / Defer +7d / Drop / Start now
```

The full action vocabulary (`defer +14d`, `defer to date`, `mit`, `change-priority`, `convert-to-project`, `move-to-backlog`, `skip`) is still available — the user can type any of those via the auto-added "Other" option. **`move-to-backlog`** parks the task indefinitely (`backlog_task.py`); surface it as an explicit option once `defer_count >= 4` (see "Default 4-option sets").

**High-defer warning**: if `defer_count >= 3` *before* this action, prepend a line in red/bold:

```
  ⚠ deferred 4 times already — drop it, or commit to a firm date?
```

Recognize when the user wants to convert to a project (see [task-project-link.md](../todo/references/task-project-link.md) conversion triggers). If `estimated_duration > 90` or the task has scope verbs, suggest `/todo turn-into-project` instead of deferring again.

## Step 5 — Hard-deadline review

Before bulk-deferring anything, scan for tasks with `hard_deadline=true`:

- Hard-deadline tasks are excluded from "defer all" bulk operations.
- For ANY hard-deadline task whose `defer_count >= 1`, pause and ask the user:
  > "T### '<name>' was deferred [N] times and still has a hard deadline of <date>. Should the hard deadline still hold, or do you want to set a more realistic date?"
- Get the user's answer before continuing. Apply their choice (keep deadline / change deadline / drop hard_deadline flag) before moving to bulk operations on that group.

## Step 6 — Forward-looking at-risk scan

After past-due is clean, scan the next **8 days** for tasks likely to slip. This is the "what's about to fall over?" pass — it runs every daily triage, even when past-due was empty.

**Deadline window (explicit user rule):** only surface an at-risk task when its `due_date` is **within 8 days** (`<= today + 8`). Flag-when-close, not early-warning: a task in danger but still 9+ days out is left alone until it gets closer. (The chronic-ignore sweep in Step 7 uses a tighter 3-day window; at-risk gets the wider 8 days because these are genuinely in danger, not just inert.)

### Stale-waiting check (runs first, alongside the at-risk scan)

A task in `status=waiting` is paused on an **external** party (a reply, a vendor, a legal review), so its slipping isn't avoidance — deferring it never raised `defer_count`. But waiting forever is its own failure mode. Run:

```
python3 ~/.agents/skills/todo/scripts/find_stale_waiting.py --pretty
```

For each task that's been waiting **more than 7 days** (`waiting_since`), nudge the user: offer to **follow up with the external party** (infer who from the `task_name`/`see_also` if you can) and ask whether to **create a check-in task** for that follow-up. A task with `status=waiting` but an empty `waiting_since` is also surfaced (we can't tell how long — stamp it now). This is the counterpart to the chronic-ignore sweep: chronic-ignore is "we're avoiding it"; stale-waiting is "someone else is sitting on it and it's time to chase them."

### At-risk criteria

Filter `tasks.csv` for rows where `status != done`, `start_date` is empty or `<= today+8`, and **any** of the following match (multiple matches = multiple reasons surfaced together). Every criterion is gated by the 8-day deadline window above — nothing with a `due_date` 9+ days out is surfaced here:

1. **Hard deadline, never started** — `hard_deadline=true` AND `due_date <= today+8` AND `status == not_started`.
2. **Hard deadline already pushed** — `hard_deadline=true` AND `defer_count >= 1` AND `due_date <= today+8`.
3. **Repeat-deferral pattern** — `defer_count >= 2` AND `due_date <= today+8`.
4. **High-priority not started** — `priority in (p0, p1)` AND `status == not_started` AND `due_date <= today+8`.
5. **Long task, no progress, due soon** — `estimated_duration >= 60` AND `status == not_started` AND `due_date <= today+8`.

**Skip tasks already handled in this same triage session** (e.g. one you just bulk-deferred in Step 3 or warned about in Step 4). Don't re-flag them; the user just acted on them.

### Presentation

- **0 hits** → say "No at-risk tasks in the next 8 days." and move on. Do not fabricate a list.
- **1–3 hits** → walk individually with the per-task prompt below.
- **4+ hits** → group first (same logic as Step 1: try by reason, then by due bucket `≤3d` / `≤7d` / `≤14d`, then by `task_type`). Briefly show the groups as a text summary (one line per group with task IDs / counts), then ask the user how to handle each group via **`AskUserQuestion`**.

### Group-level prompt (4+ hits)

Use **`AskUserQuestion`** with up to 4 questions per call (one question per group; multiple calls if there are more than 4 groups). Options per group:

```
header:   <group label, e.g. "Renewals">
question: <group description, e.g. "Renewals cluster (4): T94, T99-101, hard+def=1-2, due 6/26-6/29.">
options:  Leave as is / 1-by-1 / Defer all +7d / Drop all
```

- **`Leave as is`** is ALWAYS the first option, because at-risk tasks are flagged but not yet late — leaving them unchanged is a legitimate choice and often the right one. The user is being shown a preview, not forced to act.
- **`1-by-1`** kicks into the per-task prompt below for that group only.
- **`Defer all +Nd`** / **`Drop all`** are bulk operations. Hard-deadline tasks inside a "Defer all" still go through (per the user's explicit consent), but if the bulk push would land any task at `defer_count >= 3`, flag it in the post-action summary.
- Less common bulk operations (`Defer all +14d`, `Defer all to date`, `Convert all to project`) remain reachable via "Other".

### Per-task prompt

Ask via **`AskUserQuestion`** (see [Asking the user for per-task actions](#asking-the-user-for-per-task-actions-use-askuserquestion)). Batch up to 4 tasks per call.

Example question payload (one task):

```
header:   T148
question: **Due 7/12 (in 5d):** T148 "Draft the Q3 board update" [p1/work, def=1, HARD] — reasons: hard-deadline, never-started. What's the call?
options:  Start now / Defer +7d / Drop / Convert to project
```

`defer to date`, `mit`, `change-priority`, `skip` remain reachable via "Other".

- **start-now** — set `start_date=today` and `status=in_progress`. Use when the user commits to begin now.
- **defer +Nd / defer to date** — push `due_date`, increments `defer_count` (except no-penalty defers: `status=waiting` or non-empty `blocked_by` push without raising the count). **Hard-deadline tasks require explicit confirm** (same rule as Step 5): pause and ask whether the hard deadline still holds before deferring.
- **drop** — remove from CSV (confirms first).
- **convert-to-project** — when `estimated_duration > 90` or scope verbs (`launch`, `build`, `migrate`, `research`) — suggest `/todo turn-into-project` per [task-project-link.md](../todo/references/task-project-link.md).
- **skip** — acknowledge, no change. Use when the user is aware and chooses to leave it.

### Skipping the whole pass

If the user is in a hurry, accept `skip at-risk` (or just "skip") at the start of this step to jump to Step 7 with no changes.

## Step 7 — Chronic-ignore sweep

After the at-risk preview, sweep tasks that have rotted in the backlog — neither past-due (handled in Steps 1–4) nor about to slip (handled in Step 6), but **inert**. These are deadwood candidates. The goal is to clear them, not to revive them by default.

### Eligibility (computed by the detector)

A task in `tasks.csv` qualifies if `status != done`, its deadline is imminent or absent (`due_date` empty OR `today <= due_date <= today + 3d`), and at least one of:

**Never nag about far-off deadlines (explicit user rule).** A dated task is surfaced only once it's within **3 days** of its `due_date`. Anything further out is left alone — even if it's old and untouched — because asking about it repeatedly when the deadline is still days/weeks away is just noise. (Chronic-ignore deadwood gets this tight 3-day window; the Step 6 at-risk preview uses a wider 8-day window for tasks genuinely in danger.) Undated thin rows have no deadline to be "far from", so they stay eligible (they're the truest captured-and-forgotten deadwood). Past-due rows (`due_date < today`) are owned by past-due triage (Steps 1-4), not this sweep.

1. **`stale_21d`** — `today - last_touched >= 21d`. Primary signal.
2. **`stuck_in_progress`** — `status == in_progress` AND `today - last_touched >= 14d`. The user engaged once, then walked away.
3. **`captured_forgotten`** — `today - created_date >= 60d` AND `status == not_started` AND empty `notes`, empty `estimated_duration`, empty `project`. A thin row that's old and untouched.

Don't apply these filters in your head — LLMs are bad at calendar math. Run the script:

```
python3 ~/.agents/skills/todo/scripts/find_chronic_ignored.py
```

Outputs one JSON object per matching task (sorted by max-days-since-touch first) with `task_id`, `task_name`, `reasons[]`, `days_since_touch`, `days_since_create`, `status`, `priority`, `task_type`, `due_date`, `defer_count`, `project`, `hard_deadline`. Pipe to `--count` for a quick number, `--pretty` for human-readable.

**Skip tasks already handled in this same triage session** (deferred, dropped, started, converted in Steps 3–6). Compare against the IDs you touched earlier and drop them from the hit list before presenting.

### Presentation

- **0 hits** → "No chronic-ignore candidates." Move on.
- **1–3 hits** → walk individually using the per-task prompt below.
- **4+ hits** → group by primary reason (`stale_21d`, `stuck_in_progress`, `captured_forgotten`) and present like Step 2 with the same bulk vocabulary. Then walk groups the user picks "1-by-1".

### Per-task prompt

Ask via **`AskUserQuestion`** (see [Asking the user for per-task actions](#asking-the-user-for-per-task-actions-use-askuserquestion)). Batch up to 4 tasks per call.

Example question payload (one task):

```
header:   T82
question: **No due date:** T82 "Reach out to the prof about the reading group ..." [p3/personal, def=0] — stale_21d, 28d since touch. Recommended: Drop. Confirm?
options:  Drop (Recommended) / Revive / Start now / Defer to date
```

`convert-to-project`, `skip` remain reachable via "Other".

**Default recommendation: drop.** The whole point of this pass is to clear inertia. If the user wanted to keep it, they'd have moved on it. State "drop" as the recommended action (first option, labeled "(Recommended)"); only deviate when the task is clearly load-bearing.

Actions:

- **`drop`** — remove from CSV (confirms first). Use as default.
- **`revive`** — bumps `last_touched` to today without changing anything else. Use when the user explicitly says "yes I still care, leave it" — they'll get another 21 days before it reappears. Script: `python3 ~/.agents/skills/todo/scripts/touch_task.py <T###>`.
- **`start-now`** — set `status=in_progress` and `start_date=today` (the underlying scripts will also touch the row). Use when the user commits to begin now.
- **`convert-to-project`** — when the task is chronically ignored *because* it's too big to start. Suggest `/todo turn-into-project` per [task-project-link.md](../todo/references/task-project-link.md). Note: don't reflexively convert — converting a task the user has been avoiding doesn't fix the avoidance; sometimes drop is the honest answer.
- **`defer to date`** — push `due_date` to a real date with a real commitment. Hard-deadline rows still require the Step 5 confirmation.
- **`skip`** — no change. The clock keeps ticking; it'll surface again next triage.

### Skipping the whole pass

If the user is in a hurry, accept `skip chronic` (or just "skip") at the start of this step to jump to Step 8 with no changes.

## Step 8 — Summary

After triage:

```
Triage complete:
  - 12 tasks deferred (avg push: 7d)
  - 3 tasks dropped
  - 1 task converted to project: apply-to-conference
  - 0 tasks marked MIT
  - 4 high-defer warnings issued

  Remaining past-due: 0
  At-risk flagged (next 8d): 5 — 2 deferred, 1 dropped, 1 started, 1 skipped
  Chronic-ignore flagged: 7 — 4 dropped, 2 revived, 1 started
```

Omit the at-risk / chronic-ignore lines when those scans were skipped or returned zero hits — just note "At-risk: none" / "Chronic-ignore: none" in that case. Any integration passes you run from an extension (email, issue-tracker reconcile) add their own summary lines above these.

## Collect sub-agents, then merge their output (before you finalize anything)

If you launched any sub-agents at `triage:daily-subagents`, the run is **not**
finished until they all complete and their output has been merged in. Do this
before Step 9:

1. **Wait for every registered sub-agent to finish** and collect each one's
   result (its output markdown / artifacts). Never finalize, render, or hand the
   user any combined output of the run while a sub-agent is still running — a
   printable the run produces must contain their work. This is the hard rule.
2. **Merge each finished sub-agent's output into the run's output**, following
   the merge instructions that sub-agent's extension declares at the hook below.
   If a sub-agent failed or was skipped, say so in the Step 8 summary and proceed
   with what you have.
3. Only once all sub-agents are collected and merged do you produce or regenerate
   the run's final output(s), then move to Step 9.

Core itself produces no such output — this section only guarantees that *if* an
extension does, the sub-agents' work is folded in first. With no registered
sub-agents, this step is a no-op.

<!-- brain:ext triage:daily-merge -->

## Step 9 — Mark Morning Triage habit done

After the daily triage process completes (user has either resolved every past-due task or explicitly skipped remaining groups):

1. Look up today's Morning Triage habit row in `<brain>/tasks/habits.csv` (search for a row with name like "Morning Triage" and today's `due_date`).
2. If it exists and is NOT already `done`, mark it done via:
   ```
   brain tasks complete H<id>
   ```
   If the habit appears on an already-written agenda, update that agenda as
   part of the same workflow; see /todo SKILL.md operating principle 7.

**Why marking it matters — the brain tasks view depends on it.** The brain
tasks view shows a startup "Today's triage isn't done. Run it now?" modal
that is gated on this exact habit: config's `daily_triage_name_pattern`
(default "Morning Triage") is consumed by `check_daily_triage`, which fires
the nudge whenever today's Morning Triage habit is not `done`. Marking the
habit done is what tells the tasks view (and the agenda) that daily triage
is handled for the day, so the modal stops nagging. If you don't mark it,
the user gets nagged even though triage ran.

3. **If this pass is running as a background triage tab, signal completion.**
   When the tasks view launches daily triage in its own ephemeral tab it sets
   two environment variables in this session: `BRAIN_TRIAGE_DONE_URL` (a local
   brain-server URL) and `BRAIN_TRIAGE_TOKEN` (a one-time token). **Only when
   both are set**, after the habit is marked done, POST the completion signal so
   the tasks view auto-closes this triage tab.

   **The signal carries a `require` list — the paths of every output this run
   committed to producing that must exist before the tab may close.** Core
   itself declares none, so by default the list is empty. But this run may have
   had extension steps rendered into it (at the hooks above, and at the
   required-outputs hook just below) that produce a printable, a report, a
   digest, or similar. If any such step told you it produces an output artifact
   that must be on disk before closing, put that artifact's path in `require`.
   brain holds the signal open and will **not** close the tab until every listed
   path exists — so a premature signal can't kill the session before the run's
   declared outputs land. With an empty list the tab closes as soon as the
   signal arrives.

   Declare here any required output paths (an extension fills this in; if none
   is rendered, there are none and `require` stays empty):

   <!-- brain:ext triage:daily-required-outputs -->

   Then POST, substituting the required-output paths as a JSON string array (use
   `[]` when there are none):
   ```
   [ -n "$BRAIN_TRIAGE_DONE_URL" ] && curl -fsS -X POST "$BRAIN_TRIAGE_DONE_URL" \
     -H 'Content-Type: application/json' \
     -d "{\"token\": \"$BRAIN_TRIAGE_TOKEN\", \"require\": [<required-output paths, or empty>]}" \
     >/dev/null || true
   ```
   Do this as the very last action of the pass — once every required output
   exists the tab closes as soon as the signal lands, so anything you still need
   to show the user must come first. When the variables are unset (a normal
   in-session `/triage`, not a background tab), skip this step entirely; there is
   nothing to close.

**"Skip daily triage today" ⇒ skip the Morning Triage habit anyway
(explicit user rule).** When the user explicitly says we can **skip**
daily triage for the day — e.g. "skip daily triage", "we can skip triage
today", "no daily triage today" — that is *not* "leave the habit
pending." It is an instruction to **skip today's Morning Triage habit**
via `brain habits skip "Morning Triage"`,
with **no triage pass run**. Because Morning Triage is a **daily** habit,
that script marks today's occurrence `done` (the general
[Skipping a habit](../todo/SKILL.md#skipping-a-habit) rule) — same end state
as the completed-pass path above, reached through the one deterministic skip
script. The user has decided the pass isn't needed today, so the day counts
as triaged: skipping it stops the tasks-view startup modal nagging and keeps
the agenda's habit state honest. Do this immediately when they say to skip —
don't ask, don't run any of Steps 0–8. (This mirrors how a completed pass
ends: Step 9 is the one step that still runs.)

---

# Weekly triage

A weekly in-basket review. This is **not** just task management — it's the user's second-brain in-basket sweep. Core weekly triage processes the local scratch inbox; wire any cloud in-basket you use in through the `triage:weekly-inboxes` extension point below.

**End state: every in-basket is EMPTY.** Every item gets routed to a real home (task, note, project, or a "decide what to do with this" follow-up task).

Default to making decisions yourself (this is assistant mode). Only ask for confirmation when you are genuinely unsure of intent.

## When to invoke

- `/triage weekly` invoked directly.
- Bare `/triage` when the Weekly in-basket processing habit is due or past-due (see [Routing](#routing-when-the-user-runs-bare-triage-no-mode-argument)) AND the user picks "weekly".
- User says: "weekly triage", "in-basket processing", "process my scratch", "clear the in-basket", "weekly review".

## Step 1 — Process the local scratch inbox

1. **Read** `<brain>/scratch.md` in full (the local scratch notepad — anything the user dumped here during the week).
2. **Walk it top to bottom**, splitting into discrete items at natural boundaries (paragraph breaks, blank lines, bullet groups, headings). Treat each coherent chunk as one item.
3. **For each item, classify** as TASK, NOTE, or UNSURE — see [Classification rules](#classification-rules).
4. **Process**:
   - **TASK** → invoke `/todo` to create the task with the best-guess fields (priority, due_date, task_type, duration). If the item is clearly a project (multi-step, scope verbs like `launch`/`build`/`migrate`/`research`, multiple checkboxes), CONFIRM with the user and run `/todo turn-into-project`. When creating sub-tasks of a converted project, set `blocked_by` relationships explicitly per [task-project-link.md](../todo/references/task-project-link.md).
   - **NOTE** → place it in the appropriate `/second-brain` location (project / area / resource). Use the `second-brain` skill's decision flow. Create a new subdirectory only if no existing home fits, and ask for confirmation only when you're genuinely unsure. Pair PDFs/media with their notes per the brain conventions.
   - **UNSURE** → ask the user a single specific question: "Found '<short paraphrase>' — looks like it could be X or Y. What did you mean?" Move on to the next item while waiting if the user is batching answers.
5. **Remove the item from scratch.md** once it's been routed. Edit `<brain>/scratch.md` as you go.
6. At the end of step 1, `<brain>/scratch.md` should be **empty** (or contain only items the user explicitly chose to leave — but see [Deferring ≠ leaving in the inbox](#deferring--leaving-in-the-inbox)).

<!-- brain:ext triage:weekly-inboxes -->

<!-- brain:ext triage:weekly-linear -->

## Step 2 — Monthly check + backlog review

Every weekly triage also checks whether it's the **monthly** triage.
"Monthly" is not its own command — it's simply the **first weekly triage
of a calendar month**, and its only extra job is reviewing the backlog.

1. **Detect:** `python3 ~/.agents/skills/todo/scripts/monthly_triage_state.py`.
   If `is_monthly` is `false`, skip this step entirely. If `true`, do the
   dedupe + backlog review below, then mark it: `monthly_triage_state.py
   --mark` (so the next weekly triage this month is just weekly).
2. **Dedupe backlog vs active (monthly only, silent):**
   `python3 ~/.agents/skills/todo/scripts/dedupe_backlog.py`. This deletes
   any backlog task that has an active-list twin which was *created after*
   the task was backlogged — i.e. the user already re-created (revived) it
   by hand, so the backlog copy is a stale duplicate. It does nothing but
   delete the duplicate backlog row, and prints nothing. **Silent like the
   purge: don't announce what (if anything) was deduped.** Run it before
   the backlog review so resurfaced candidates are dup-free.
3. **Backlog review (monthly only):** `python3 ~/.agents/skills/todo/scripts/list_backlog.py --pretty`.
   The goal is **resurfacing**, not clearing: surface only backlog items
   that (a) look **relevant to current work** and (b) look **doable given
   current time/demands**. This is how the user rediscovers tasks they
   parked when they were irrelevant or there was no time — that might
   matter now. **Do NOT walk every backlog item** — that defeats the
   purpose and wastes the user's time. Pick the handful that genuinely
   merit a second look and ask, via `AskUserQuestion`, whether to
   **leave it parked** or **restore** each (`backlog_task.py <T###>
   --restore`, then set a fresh `due_date`/`priority`). **`Leave parked`
   is ALWAYS the first option** — the backlog default is to stay parked,
   and surfacing an item here is a preview, not a nudge to restore it
   (same reasoning as Step 6's `Leave as is`). Items that aren't
   obviously relevant-and-doable stay in the backlog silently; the
   6-month purge (Step 0) eventually clears the truly dead ones with no
   prompt.

## Step 3 — Mark the Weekly habit done

After **all** in-baskets are empty:

1. Look up the Weekly in-basket processing habit in `<brain>/tasks/habits.csv` (current row name: "Weekly in-basket processing").
2. If it isn't already `done`, mark it done via:
   ```
   brain tasks complete H<id>
   ```
   If the habit appears on an already-written agenda, update that agenda as
   part of the same workflow; see
   [Daily triage Step 9](#step-9--mark-morning-triage-habit-done).

## Step 4 — Offer to chain into daily triage

Once the weekly habit is marked done, ASK the user once:

> "Weekly triage complete — the in-baskets are empty. Want to run daily triage now?"

- If yes → start the [Daily triage](#daily-triage) workflow from Step 1.
- If no → end here.

---

# Shared rules

## Never auto-mutate

Triage never deletes, defers, re-prioritizes, or routes anything without an explicit user choice. Bulk options count as explicit consent for the **entire group**. In weekly mode, your best-guess routing of a TASK or NOTE counts as a decision the user has implicitly authorized — but anything UNSURE must be confirmed.

## Asking the user for per-task actions (use AskUserQuestion)

Whenever you offer the user a choice between concrete actions — **any** "what should I do with this?" / "want me to X or Y?" prompt — **use the `AskUserQuestion` tool**, not a free-form text prompt. It's faster for the user (clickable options) and lets you batch.

This is **not** limited to the 1-by-1 walks in Steps 4, 6, and 7. Those are the *common* cases, not the whole list. The rule covers **closing and follow-on offers just as much as in-step walks** — e.g. "should I convert any of these to projects?", "want me to slot these into the agenda or leave them?", "drop this stale one or keep it?", offering a project conversion, offering to reschedule something. The tell is simple: **if the answer is the user picking among options, it's an `AskUserQuestion` call.** If you catch yourself typing a prose question that ends in two or more choices (even "yes / no / which"), stop and reach for the tool instead. The end-of-triage summary is a frequent offender: a wrap-up that ends with "want me to do A, or B?" must put A/B in `AskUserQuestion`, not prose.

**The question and its options must be self-contained.** The user has explicitly said they will NOT read long stretches of your reasoning in triage/agenda flows — the transcript is long and dense. So do not put the actual choice or the information needed to decide it in prose *above* the tool call where it can be skipped. Lead-in prose must be terse (a line or two), formatted to stand out (bold, a table, or a `---` rule), and never the place where the real question lives. Put the decision, the context, and the trade-offs **inside the `question` field and the option labels/descriptions** so everything the user needs is in the box they actually look at. If you find the explanation doesn't fit, shorten it — don't spill it into skippable prose.

**Batching rule:** `AskUserQuestion` supports up to 4 questions per call. **Always pack up to 4 tasks into a single call** when walking 5+ tasks in a row. Don't ask one at a time.

**Question shape** — one question per task. **Lead with the deadline, before the task ID — always.** Start the `question` field with the due date in **bold**, because knowing instantly whether a decision is about something due *today* vs *weeks out* is the single most important cue for fast triage. Format:

- Due today → `**Due TODAY:** T### '<name>' [p0/work, def=1] …`
- Future → `**Due 7/15:** T17 '<name>' [p2/personal, def=3] …`
- Past-due → `**Due 7/5 (2d late):** T### …`
- No due date → `**No due date:** T### …`

After the bolded deadline, give the task ID, name, type/priority/age, and key flags (defer_count, hard_deadline). Use the task ID as the `header`. This deadline-first rule applies to **every** per-task prompt — Step 4 past-due, Step 6 at-risk, Step 7 chronic — and to any group-level line that names individual tasks. Pick the 4 most context-appropriate options from the per-step menu (see below); "Other" is auto-added by the tool, so the user can type free-form for less common actions (`defer to date`, `change priority`, `mit`, `convert-to-project`, etc.) without you having to list them.

**Default 4-option sets by step:**

- **Step 4 (past-due 1-by-1):** `Done` / `Defer +7d` / `Drop` / `Start now`
- **Step 6 (at-risk 1-by-1):** `Start now` / `Defer +7d` / `Drop` / `Convert to project`
- **Step 7 (chronic-ignore 1-by-1):** `Drop` / `Revive` / `Start now` / `Defer to date`

Deviate from these defaults when context obviously warrants it (e.g. show `Defer +14d` instead of `+7d` when the user has been deferring +14 repeatedly this session).

**`Move to backlog` is a standing action everywhere.** It's always reachable via "Other," and you should **surface it as an explicit option whenever `defer_count >= 4`** — at that point deferring again is the wrong default, so swap `Defer` for `Move to backlog` in the option set (e.g. Step 4: `Done` / `Move to backlog` / `Drop` / `Start now`). Backlogging parks the task (clears its dates, hides it from active views) via `backlog_task.py <T###>`; it stops the per-triage nagging without losing the task. **If the task belongs to a project, run the project follow-up** (backlog whole project? archive it?) per /todo SKILL.md "Backlog ↔ projects" — ask with `AskUserQuestion`, don't assume.

**Group-level prompts:**

- **Step 2 (past-due groups)** — prose menu is fine. The past-due flow is high-velocity and the user typically answers for several groups in one message ("work: 1-by-1, personal: defer all 7d, …").
- **Step 6 (at-risk groups)** — **DO use `AskUserQuestion`**, with `Leave as is` always present as the first option. At-risk tasks aren't late yet, so leaving them alone is a valid choice and often the right one. See Step 6's "Group-level prompt (4+ hits)" subsection for the exact shape.
- **Step 7 (chronic-ignore groups)** — `AskUserQuestion` is fine but not required. The default action is `Drop all` (the whole point of the pass is to clear inertia); make that the first option labeled "(Recommended)".

**What does NOT use AskUserQuestion — this list is exhaustive.** Everything else that offers a choice uses the tool. Do not invent new prose-question exemptions; if a prompt isn't on this list and its answer is the user choosing among options, it's an `AskUserQuestion` call.

- **Hard-deadline confirmations** (Step 5, and the in-step warnings in Steps 6/7) — keep these as a single sharp prose question. They're not "pick an action," they're "is this still real?"
- **High-defer-count warnings** (Step 4 `defer_count >= 3` line) — prepend the warning line, then still ask via `AskUserQuestion`. The warning is context, not a separate question.
- **In-basket UNSURE clarifications** (weekly Step 1) — "I found '<x>', did you mean A or B?" is an open intent question, not a fixed action menu; prose is fine. (If it does collapse to a clean A/B/C action choice, use the tool.)

## Classification rules

- **TASK** — imperative phrasing ("Send X to Y", "Email so-and-so", "Fix the Z bug"), TODO-list bullets, named actions, anything that requires the user to *do* something.
- **NOTE** — reference content, links, quotes, recipes, code patterns to remember, half-formed ideas, design sketches. Static knowledge, not action.
- **UNSURE** — terse fragments, opaque code blocks, single words, weird syntactic things, anything that could plausibly be either. ASK the user. Don't guess.

This is an in-basket of the user's random thoughts captured at random times. Some of it won't make sense. **It is fine to be unsure** — what's NOT fine is asking for confirmation when you're already sure. Asking too much wastes the user's time.

## Deferring ≠ leaving in the inbox

If the user can't decide what to do with a note or task right now, **the inbox is still not where it stays.** Deferring means creating a TASK in `tasks.csv` whose name is the deferred decision itself — e.g.:

> `T###` "Decide what to do with the 'multi-language querying' demo idea — see scratch capture from 2026-06-10"

…and removing the original item from the inbox. The inbox empties out; the decision moves into the task system where it can be triaged like any other open loop.

## Reference

See [references/heuristics.md](references/heuristics.md) for the original daily-triage heuristics document (preserved for historical reference and richer examples). The task ↔ project linkage rules used during weekly TASK routing live in [/todo's task-project-link.md](../todo/references/task-project-link.md).
