# Task-project linkage

This is the canonical reference shared by [`/projects`](../SKILL.md),
[`/todo`](../../todo/SKILL.md), and triage. Tasks and habits live in the
selected workspace's `tasks/{tasks,habits}.csv`; project metadata lives under
`projects/<namespace>__<outcome>/` or `archive/projects/<slug>/`.

## Atomic work

One task row is one unit of work. A task needing distinct sub-tasks is a
project: decomposition belongs in its project folder and linked atomic tasks,
not `- [ ]` scaffolding inside a task's `notes`. Sequential work sessions of
the same task can use todo's native chunking instead.

## Forward link

The task/habit `project` field is the full namespaced project slug. Empty
means unassigned. Use native task add/edit commands through `/todo`; never mutate
a CSV directly.

## Reverse link

The project's `.METADATA.json:tasks[]` lists linked short IDs (`T17`, `H42`).
Native commands maintain both directions. Immutable task UUIDs remain stable
when a short display ID changes; do not invent replacement identifiers.

## Validation

`brain tasks lint` validates task/project links, including archived projects.
It reports missing project folders and reverse IDs that are absent or point
elsewhere. It never silently clears an orphan task's project. `--fix` can
add missing reverse links for existing forward links; a reverse-only mismatch
requires a decision rather than guessing which side was intended. Follow
[todo's native sync rules](../../todo/references/sync-rules.md).

## When to convert

Offer conversion for explicit multi-step requests, sub-task checkboxes,
large estimates (over 90 minutes), repeated deferral (`defer_count >= 3`), or
scope verbs such as launch/build/migrate/research. These are prompts to inspect
scope, not proof: converting avoided work does not resolve avoidance.

## Convert a task

For `/todo turn-into-project <task>`:

1. Confirm the source task and propose a namespaced outcome slug using
   [project naming](../SKILL.md#namespaces-and-metadata).
2. Use [Create](../SKILL.md#create) and `brain project new` to create the
   scaffold. Resolve due date, starting status, and required priority. The
   parent task's priority is a useful proposal, not a silent choice.
3. Propose a manageable set of atomic child tasks (usually 3 to 10). Include
   names, priority, due dates across the project horizon, task type, and
   realistic estimated duration. Explicitly identify the dependency chain
   using `blocked_by`, including multiple blockers when needed. Let the user
   confirm, edit, add, or remove children before creating them.
4. Create approved children with native `brain tasks add` commands through
   `/todo`. Set `project=<slug>` and the relevant `blocked_by` values. Native
   task mutations own the reverse array; do not populate it by hand. Verify
   both directions using the native lint command and report partial creation.
5. Only after the project and approved children exist, preview source removal
   with `brain tasks remove`, obtain consent for dependency and linked-issue
   policies, and apply with `--yes` and the preview fingerprint as
   `--expect-plan`. Read [Remove tasks](../../todo/SKILL.md#remove-tasks).
   The CLI owns reference cleanup. If blocked or failed, report incomplete source removal
   and the operation ID when available; keep the created project and children
   visible. Do not retry a committed deletion just because agenda refresh warns.
6. Verify the resulting project and link state. Native project commands already
   rebuild their lookup; use `brain tasks lint` for linkage, not cloud sync.

The completed conversion has linked children, matching reverse links, explicit
sequential dependencies, and an honest source-removal outcome. This applies
whether invoked through todo, triage, or another workflow.

## Linkage commands

These todo routes use its native command implementation:

| Route | Result |
| --- | --- |
| `/todo link <task> <project>` | Sets the forward link and maintains the reverse link |
| `/todo unlink <task>` | Clears both sides |
| `/todo add-to-project <project> "<text>"` | Creates a task with the project prefilled |
| `/todo project-tasks <project>` | Lists open tasks and recently completed tasks |
| `/todo project-status <project>` | Summarizes done/total, MITs, past-due work, and estimated effort |
| `/todo orphans` | Runs native lint without repair |

## Archive

Before [archiving](../SKILL.md#archive), resolve each open linked task explicitly:
re-home to another project, keep but unlink, or complete natively. An explicit
request to delete tasks uses the preview/consent/removal contract, never a file
edit. Completed tasks retain their archived project slug and history; habits
retain completed occurrences according to native retention rules. No silent
loss or orphaning is acceptable.

## Read task to project

Resolve the exact `project` slug in the selected workspace, checking active
and archived project locations. For task listings/status use native todo routes
or read-only CSV filtering by `project`; do not mutate via a query script.

## Read project to tasks

Read `.METADATA.json:tasks[]` and resolve each ID exactly in tasks/habits.
Do not substitute fuzzy string matching for the structured relationship.
