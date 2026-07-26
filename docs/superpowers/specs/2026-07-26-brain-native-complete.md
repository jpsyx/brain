# Native `brain tasks complete`

## Status

Planned from the §19 sync backlog. This feature moves task/habit completion
into the `brain` binary and removes the bundled `mark_done.py` completion path.

## Problem

`brain tasks complete`, the TUI mark-complete action, and `POST /habits/done`
all depended on a bundled Python `mark_done.py` script. That made completion a
runtime script dependency even though completion is structured CSV work: set a
row done, stamp `completed_date`/`last_touched`, spawn the next habit occurrence,
and migrate `mit` to the next chunk.

Users should only need the installed `brain` binary. They should not need Rust,
Python scripts, or a checked-out repo to mark an item done.

## Goals

- Implement completion in Rust inside `src/tasks/complete.rs`.
- Keep `brain tasks complete <id>` as the user and agent CLI.
- Reuse the same Rust API from the TUI and `/habits` server route.
- Support task IDs, habit IDs, bare IDs with ambiguity detection, and fuzzy name
  matching.
- Preserve key completion behavior: `status=done`, `completed_date=today`,
  `last_touched=today`, habit recurrence spawn, `.habits_next_id`, and chunked
  MIT migration.
- Remove the bundled `mark_done.py` script from the skill payload.
- Update brain skills, global skills, and global rules so direct references
  point to `brain tasks complete <id>`.

## Non-Goals

- No new CLI command surface beyond the existing `brain tasks complete`.
- No requirement for users to install Rust.
- No Linear MCP integration inside the binary.
- No full Rust port of every remaining Python mutator in the todo skill.

## Docs Contract

Update:

- `docs/features.md` for user-visible completion behavior.
- `docs/integrations.md` for the removal of the script handoff.
- `docs/architecture.md` for the module/data-flow map.
- `docs/decisions.md` for the native completion rationale and `/habits` reuse.
- `docs/superpowers/brain-sync-status.md` when complete.
- Bundled skill docs under `skills/` and any direct global-skill/global-rule
  references.
