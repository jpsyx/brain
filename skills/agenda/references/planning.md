# Agenda planning

## Date and state

Run `date +%F` and `date` at each build/update. Resolve today, tomorrow, or an
explicit date from that live clock, not the session's opening date. Distinguish
the target date from today when planning future days. Resolve the configured
Markdown destination before deciding whether this is a first build or an update;
see [artifacts](artifacts.md).

Read the selected workspace's `tasks/tasks.csv` and `tasks/habits.csv` with a real
CSV parser. Task rows must have status other than `done` or `backlog`, and empty
`start_date` or `start_date <= target date`. Future starts never enter a bucket,
MIT callout, Suggested order, or Cut order. Respect blockers; a dependency cannot
be performed before its prerequisite. Keep actor assignments intact.

Habits eligible for that date have pending status and empty or elapsed due date;
include done-on-target-date rows only in reference/completion sections. Honor
recurrence and `ideal_time`; do not invent habit names or IDs. Read definitions
from [task schema](../../todo/references/schema.md) when needed. Bucket tasks into
MITs, hard deadlines due by the target date, hard deadlines in the following week,
in-progress, due-on-target-date, and chunk families. Run `brain tasks stale-waiting`
for external waits older than the configured threshold; recommend a follow-up or
check-in task without sending anything. With 10+ overdue tasks, delegate their
review to `/triage` when needed, rather than duplicating triage inside this plan.

## First build and explicit regeneration

Treat the first plan as a collaborative draft until the user locks it in. Ask only
high-impact unresolved questions, batched on the frontend's available question
surface or in concise text when no question tool exists. Include the task ID,
name, deadline, recommendation, and tradeoff in the question itself.

For today's first build, compare pending habits' actual `ideal_time` or explicit
windows to the live time. Ask once about habits whose windows already passed;
complete only those the user confirms through `brain tasks complete <habit>`.
No fixed morning-hour threshold and no assumed off-system completion. Do not ask
this for a future date. For hard-deadline tasks with `defer_count >= 1`, verify
whether the deadline remains real before treating it as immovable or proposing
a defer. Batch genuinely unresolved scope choices with these questions.

An existing plan defaults to an update: preserve choices already settled and skip
first-build questions. Only an explicit request to start over re-enters full
planning. A new material conflict may justify one targeted question.

## Capacity and placement

1. Reserve the explicit [anchors and calendar](../SKILL.md#daily-anchors) first.
   Use the user's available day/window and the live remaining time for today.
   Ask if the available time cannot be inferred and changes feasibility.
2. Apply a work/personal partition only when explicitly configured. Otherwise
   schedule all tasks by priority/deadline. A habit's `ideal_time` takes precedence
   over a tag partition. Respect explicit outside constraints; explain a needed
   exception to an established window instead of quietly breaking it.
3. Place required habits only when the workspace explicitly marks them required,
   then MITs and hard deadlines, then the chunk minimum below. Use the earliest
   feasible suitable slots; no default morning work block or code-work quota.
4. Fill remaining eligible work by priority, fitting real durations and leaving
   transitions/breaks realistic. Use estimates (`quick=5`, `short=15`, `medium=30`,
   `long=45`, `very_long=60+` minutes) only when a confident estimate suffices.
   Ask only if uncertainty changes the decision.
5. If work will not fit, say so, recommend the exact drop/defer/reduced scope,
   and obtain a decision before a consequential change. Never silently overstuff,
   move hard deadlines, or omit obligations. No default bedtime or hard stop.
6. Deliberately leave free time unfilled after all today's eligible work is placed.
   Do not pull at-risk work forward or add chunks to fill it unless asked.

## Chunk planning

Group `<base> (i/N)` rows by base and total. Let `R` be unfinished chunks and
`days_remaining` the inclusive days through the family deadline. Surface the
minimum `ceil(R / days_remaining)` chunks, or all `R` if due today/past due.
Walk from the lowest unfinished number, respecting prerequisite order. For an
undated family, select the next actionable chunk rather than inventing a deadline.
If that minimum cannot fit alongside hard deadlines/MITs, recommend changing the
deadline, cutting lower-priority work, or explicitly accepting lateness. Chunks
beyond the minimum stay in the store, outside the plan's three action sections.
Native completion transfers MIT to the next sibling and syncs the occupied slot;
recalculate capacity only when the user requests a plan revision. Creation and
defer cascades remain [todo's responsibility](../../todo/SKILL.md#chunked-tasks).

## Cut order

Choose at most five droppable items from Suggested order, in drop-first order:
recently revived chronic items, lower priorities without hard deadlines, vague
large tasks, repeatedly deferred work, then optional habits as a last resort.
Never cut MITs, hard-deadline-today items, or explicitly mandatory habits. Pair
all task/habit IDs with names. Render using [the artifact contract](artifacts.md).
