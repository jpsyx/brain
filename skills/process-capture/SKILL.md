---
name: process-capture
description: Process explicitly selected Brain capture items or the capture pass of weekly triage; inspect every input, regroup by idea, route notes and actions, and account for incomplete processing.
---

# process-capture

Process `capture/` when the user asks to file/process its material or during
[`/triage weekly`](../triage/SKILL.md#weekly-triage). Reading an inbox for
retrieval is not permission to tidy, rename, or process it. Respect a named
selection; do not expand a one-item request into an entire-inbox sweep.
Plain capture processing does not complete the weekly triage habit.

## A file is not a note

The unit of filing is an idea. Several files can be one idea; one file can
contain several unrelated ideas; one coherent file may be one note. Read
before classifying. User subdirectories hint at relationships, not the PARA
destination schema. Never mirror an inbox folder tree automatically.

## Classify and route

| Kind | Owner and result |
| --- | --- |
| Note | [`/organize`](../organize/SKILL.md) places it in PARA |
| Action | [`/todo`](../todo/SKILL.md) creates a native task with sensible priority, due date, type, and duration |
| Multi-step outcome | [`/projects`](../projects/SKILL.md#create), with [task-project linkage](../projects/references/task-project-link.md) for conversion |
| Both note and action | File the note and create the task; put the note path in the task's `see_also` |
| Neither | Explain why; ask before destroying anything, including a duplicate |

Capture creates tracked actions; it does not execute the work described by a
captured task. Follow the user's explicit destination. Otherwise choose the
obvious placement and state it. Ask only about unresolved consequential
ambiguity, destructive disposition, or a new configured namespace.

## Process

1. Inventory every selected input recursively, including nested directories
   and non-Markdown files. Keep an input-to-result accounting list throughout
   the pass so an interruption does not hide unfinished material.
2. Read text in full; inspect PDFs and images; identify and inspect audio,
   video, archives, and other formats with available tools. Do not classify
   from filenames or silently skip unreadable files. Record exactly what
   could not be inspected and why.
3. Regroup into ideas: merge related inputs and split unrelated fragments.
   Track every part of a split back to its source.
4. Classify and route each idea with the owners above. Organize handles media
   pairing, substantive-source summaries, and useful neighbor links. Keep
   original resource artifacts with their commentary.
5. Verify the destination artifact and any task creation succeeded before
   removing a source. Move routed files out of `capture/`; after splitting,
   remove the source only when every fragment has safely landed and any
   destructive disposition is authorized. Clear one completed input at a
   time for recoverability. Never discard an unreadable or failed input to
   make the inbox appear empty.
6. If the user cannot decide now, create a task naming the decision and move
   the material into a safe real home once that home is established. The
   unresolved decision is tracked, not buried back in the inbox. If inspection
   or safe placement is blocked, retain the input and report the blocker.
7. Report every input's destination, resulting task IDs, unresolved decisions,
   and unreadable/failed items with reasons. An empty inbox is a result only
   when every input is accounted for; partial processing remains partial.
