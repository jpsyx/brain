---
name: agenda
description: Build or revise a daily agenda with realistic capacity, configured anchors, task and habit state, and a printable PDF. Use for planning a day or regenerating its plan; task lists, counts, and next-task recommendations stay with todo.
---

# agenda

Own plan composition for the selected `BRAIN_ROOT` and effective `BRAIN_ACTOR_ID`.
The workspace contract supplies shared rules and response presentation.
`/todo agenda` and `/todo plan-day` are compatibility aliases for `/agenda`.

## Choose the operation

| Request / state | Behavior |
| --- | --- |
| First build: no file for the target date | Read [planning](references/planning.md); produce a collaborative draft and [artifacts](references/artifacts.md). |
| Routine update: a plan already exists | Preserve settled choices; reread current data and revise only the requested plan, without replaying first-build questions. Render the revised artifacts. |
| Native mutation: complete, defer, skip, add, remove, assign, or edit a row | Use [todo's native commands](../todo/references/commands.md). Rust synchronizes the existing agenda and refreshes an existing PDF. Do not rewrite either artifact yourself. |
| Explicit regeneration: user says rebuild from scratch | Re-enter full planning even if a file exists, retaining real task state and explicit constraints. |

Read only the relevant reference. Building an agenda does not complete daily
triage. An explicit skip of daily triage delegates to
[triage's keyed skip](../triage/SKILL.md#skip-daily-triage-today--skip-the-morning-triage-habit-run-nothing-else).
With no extension, there are no named mandatory habits, quotas, personal windows,
calendar integrations, or cutoff defaults. Use actual rows, `ideal_time`, recurrence,
and constraints the user supplied.

## Daily anchors

Apply explicit fixed blocks before task placement. An extension may supply windows,
work/personal tag mapping, required habits, or other constraints. No contribution
means no extra anchors and no work/personal partition.

<!-- brain:ext agenda:anchors -->

## Calendar busy blocks

Read `brain config get calendar_id`. When configured and a usable integration is
supplied, fetch busy blocks for the target date on every build/update. Reserve their
full duration, including partial-day events. Do not schedule tasks inside them.
Render them as unnumbered, unchecked lines in chronological position. If no source
is available, state that calendar occupancy is unknown; do not invent availability.
With no configured calendar/integration, skip the query.

<!-- brain:ext agenda:calendar -->

## Work-hours cutoff

Honor an explicit cutoff while planning. After placement, apply any configured
tracking/warning behavior. No contribution means no cutoff action.

<!-- brain:ext agenda:cutoff -->

## After build

After writing and verifying the Markdown and PDF, apply any contributed post-build
steps. No contribution means no extra artifacts or actions. If this run is part of
a managed skill session, pass any required outputs as runtime completion data.

<!-- brain:ext agenda:after-build -->
