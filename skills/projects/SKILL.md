---
name: projects
description: Create, update, complete, harvest reusable work from, or archive a Brain project; maintain task/project consistency and convert multi-step tasks into projects.
---

# projects

Own the lifecycle of outcome-driven, finite work in `projects/`. Use native
`brain project` commands for metadata and archive mutations. Tasks and habits
remain owned by [`/todo`](../todo/SKILL.md); read the canonical
[task-project linkage](references/task-project-link.md) when creating child
tasks, converting a task, or retiring a project with linked tasks.

## Namespaces and metadata

A project slug is `<namespace>__<outcome-slug>`: lowercase letters, digits,
and hyphens within each part, with the double underscore separating them.
Name the outcome, such as `work__write-usage-doc`, rather than a topic.
Use the acting person's configured namespaces from the loaded workspace
context. Choose the obvious existing namespace and state it; ask about an
ambiguous choice or a new configured namespace before creating the project.
A hyphenated namespace such as `side-project` is valid.

Read [metadata](references/metadata.md) for fields and status/priority meanings.
`.METADATA.json` is canonical; `projects-lookup.csv` is derived. The README
has an H1 title, a short description of the outcome, and useful working-note
pointers, without duplicate status/due metadata.

## Create

For `/projects new` or "start a project for X":

1. Choose the namespace and outcome. If only a topic was supplied, propose
   an outcome or ask what success means. A responsibility with no finish line
   belongs with [`/organize`](../organize/SKILL.md), not in a new project.
2. Resolve missing due date, starting status, and priority with the user.
   Priority is required (`p0` through `p4`); do not silently default it.
   Suggest `not-started` if work has not begun or `in-progress` if it has.
   Use an absolute `YYYY-MM-DD` due date, or `none` when there is no deadline.
3. Create the scaffold:

   ```sh
   brain project new <namespace>__<outcome> --title "<title>" --priority <p0-p4> --status <status> --due <YYYY-MM-DD-or-none> --description "<description>"
   ```

   The command validates fields, refuses an existing folder, writes metadata
   and README, and rebuilds the project lookup. Put supplied working material
   in separate notes alongside them. Report the created project and its location.

## Update

Use `brain project show <slug> --json` to inspect current state and
`brain project set <slug>` with `--title`, `--status`, `--priority`, or `--due`
for changes. It preserves unknown fields and rebuilds the lookup. Do not edit
metadata or lookup rows by hand or run redundant reindex after this command.
Completion must pass the checks below before setting `done`.

## Complete

For `/projects complete` or "mark this project as done":

1. Identify the project and read its state. If already `done`, report the no-op.
2. If it has not reached `extracting-ips`, offer
   [IP extraction](references/extract-ips.md) before completion. Only skip
   extraction when the user confirms there is nothing reusable to harvest.
3. Inspect `projects/<slug>/ips/` if it exists. List the staged material and
   resolve its disposition: promote approved pieces, pause for manual review,
   discard only with explicit consent, or honor the user's explicit choice
   to retain the staging directory with the archive. Do not silently abandon
   staged work or set `done` while disposition is unresolved.
4. Once extraction and staging are resolved or explicitly waived, run:

   ```sh
   brain project set <namespace>__<outcome> --status done
   ```

   Report completion and that the project is ready to archive. Completion
   alone does not authorize deleting its files or open tasks.

## Extract IPs

For `/projects extract-ip`, read [Extract IPs](references/extract-ips.md).
Stage copies from one project for review, then promote only approved pieces.
Reusable notes, outlines, templates, drafts, diagrams, and research artifacts
can become Intermediate Packets (IPs). Keep project-specific working material
with its project; promote independently useful material to areas or resources.

## Archive

For `/projects archive` or "I'm done with X":

1. Resolve the project, then run [Complete](#complete) if it is not `done`.
2. Run `brain project show <slug> --json`. Use its native `died_quietly`,
   `open_tasks`, and `ignored_tasks` findings, rather than reconstructing a
   chronic-ignore time window. If it died quietly, surface that before
   changing linked tasks so archiving is a deliberate decision.
3. Read [task-project linkage](references/task-project-link.md#archive).
   Resolve each open linked task with the user: re-home, keep but unlink,
   or mark done through native task commands. Preserve completed history
   and its link to the archived slug.
4. If the user explicitly asks to drop open tasks, follow
   [/todo's removal contract](../todo/SKILL.md#remove-tasks): preview the exact
   IDs, obtain consent for dependency and linked-issue policies, and apply
   one batch with `--yes` and the preview fingerprint as `--expect-plan`.
   The CLI owns reference and project-link cleanup. Report incomplete
   deletion and the operation ID when available before proceeding; do not
   patch CSV rows or metadata arrays by hand.
5. Run `brain project archive <namespace>__<outcome>`. It moves the entire
   folder into `archive/projects/<slug>/`, repoints its metadata, and rebuilds
   the lookup. Report the archived location and any unresolved task outcome.
   Do not substitute a manual directory move for the native command.
