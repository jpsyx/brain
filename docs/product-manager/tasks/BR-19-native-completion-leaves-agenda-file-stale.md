---
id: BR-19
title: Native task/habit completion doesn't sync the agenda markdown, so it silently goes stale or gets corrupted
status: done
priority: high
assignee: jpsyx
labels: [bug]
estimate:
project:
milestone:
cycle:
parent:
github:
blocked_by: []
created: 2026-08-24
updated: 2026-08-24
---

# BR-19: Native task/habit completion doesn't sync the agenda markdown, so it silently goes stale or gets corrupted

## Description

Today's `/tmp/<today>.md` (the day's printable agenda) turned up missing its
title, `**Load:**`/`**Bottom line:**` lines, the MIT callout, Suggested
order, and Cut order — only "Today's habits" and "Completed today" survived.
The regenerated PDF built from that truncated file was missing the whole
actionable body.

The initial suspect was `update_agenda_on_mutation.py`'s `touch` action
(called right before the corruption was noticed), but reading `main()`
clears it: the `touch` branch only re-renders the Habits/Completed sections
(`_render_today_habits_section` / `_render_completed_today_section`) and
never touches `sections[idx]` for MIT/Suggested/Cut, so `_join_doc` should
reassemble every other section byte-for-byte from `_split_sections`'s
output. Nothing in that path explains the loss.

The better-supported explanation: **native completion never syncs the
agenda file at all.** `mark_task_complete` in the TUI (and, by extension,
`brain tasks complete` on the CLI) only calls
`complete_in_workspace_for_actor_with_today(...)` to mutate the CSV, then
`reload_tasks()` — there is no call anywhere in that path to
`update_agenda_on_mutation.py` or any Rust equivalent. The *only*
documented mechanism for the agenda to reflect a native completion is
`/todo`'s operating principle 7 telling an LLM session to notice and
rewrite `/tmp/<today>.md` by hand — which is exactly the kind of freehand
edit that can drop sections if the session doing it isn't careful to
preserve everything untouched. Today's file most likely went stale/wrong
because something completed T535 (and habits H304/H311) natively, and
whatever rewrote the agenda afterward didn't preserve the other sections.

This is a real gap regardless of which specific incident produced today's
file: there is no deterministic, tested code path that keeps `/tmp/<today>.md`
in sync after a *native* completion, only a fragile "hope the LLM
remembers and does it correctly" convention.

## Acceptance criteria

- [x] A failing test reproduces the gap first: complete a task or habit via
      the native path (`complete_in_workspace_for_actor_with_today` /
      `mark_task_complete`) against a fixture agenda file with a full set of
      sections (title, Load, Bottom line, MIT, Suggested order, Cut order,
      Today's habits, Completed today) and assert the file is either (a)
      left completely untouched, or (b) updated the same way
      `update_agenda_on_mutation.py`'s `done` action would update it —
      whichever the design lands on (see Notes below for the two options).
- [x] Native completion (TUI `mark_task_complete` and CLI `brain tasks
      complete`) reliably keeps `/tmp/<today>.md` in sync with the same
      guarantees `defer_task.py`/`touch_task.py`/the `done` mutator give:
      MIT callout, Suggested order, and Cut order lines for the completed
      id are dropped/renumbered (or swapped for the next chunk), and
      Today's habits + Completed today are re-derived from the CSVs —
      with every other section (title, Load, Bottom line, and any
      unrelated content) byte-for-byte preserved.
- [x] No freehand LLM agenda rewrite is required for this case anymore;
      `/todo` SKILL.md operating principle 7 is updated to reflect whatever
      the fix does automatically vs. what (if anything) is still left to
      the agent.
- [x] `docs/features.md` (or wherever agenda-sync behavior is documented)
      describes the native-completion agenda-sync path.

## Notes

### Pointers (as of 2026-08-24)

- `src/tui/app_actions/commands.rs:243` (`mark_task_complete`) — the TUI's
  native completion entry point. Currently: mutate CSV, `reload_tasks()`,
  nothing else. This is the gap to close.
- `src/tasks/complete/mod.rs:103`
  (`complete_in_workspace_for_actor_with_today`) — the actual completion
  logic both the TUI and (presumably) the `brain tasks complete` CLI call
  into; check `src/tasks/cli.rs` for the CLI's call site too.
- `skills/todo/scripts/update_agenda_on_mutation.py` — the existing,
  tested, deterministic agenda-sync logic for the *script-driven* mutators
  (`defer_task.py`, `defer_habit.py`, `touch_task.py`). `main()`'s
  `done`/`defer` branches (~line 408) are the reference behavior to match
  or reuse for native completion — either shell out to this script from
  Rust, or port the section-preserving logic
  (`_split_sections`/`_join_doc`/`_drop_lines_with_id`/
  `_render_today_habits_section`/`_render_completed_today_section`) into
  Rust so both paths share one implementation instead of two.
- `skills/todo/SKILL.md` operating principle 7 (~line 197) — currently
  documents the freehand-LLM-rewrite convention as the expected path after
  `brain tasks complete`; needs updating once native sync exists.
- `skills/todo/scripts/tests/test_workspace_context.py` — existing test
  patterns for `update_agenda_on_mutation.py` (e.g.
  `test_agenda_mutation_inserts_core_sections_before_generic_optional_content`)
  to follow for the new regression test, whichever side of the fix ends up
  owning it.

### Design decision (resolved 2026-08-24)

**(a) — native completion actively syncs the agenda**, and the logic was
**ported to Rust** rather than shelled out to the Python script.

Why (a) over (b): the agenda is not a report you regenerate on demand. The
user works off it, and prints it, all day. A completion that visibly leaves
a done task in "Suggested order" trains the user to distrust the file, and
"the next build will fix it" can be hours away. (b) also could not satisfy
acceptance criterion 2 as written.

Why the port over shelling out: the sync is now on brain's own completion
path, so depending on a bundled skill script being installed — and on a
`python3` — for a guarantee the binary makes about its own mutation inverts
the dependency (the skill is brain's output, not its runtime). In Rust the
decision is a pure function over parsed markdown and CSV rows, which is
where this repo tests things. `brain tasks sync-agenda` then exposes it, and
`update_agenda_on_mutation.py` became a thin delegator, so there is exactly
one implementation instead of two.

### Log

- 2026-08-24 **closed.** Shipped in 0.74.0 (`fix(tasks): sync the day's
  agenda on native completion`). New `src/tasks/agenda/` owns the
  section-preserving sync (pure `doc`/`lines`/`derive`/`sync` + a
  best-effort `io` shell); `complete::complete_and_sync_agenda` is the one
  native entry point for both the CLI and the tasks view; `brain tasks
  sync-agenda [<id>] [--action done|defer|touch] [--date …]` exposes it;
  `update_agenda_on_mutation.py` delegates to that (and its `backlog` /
  `restore` actions now work at all — argparse used to reject them, so
  `backlog_task.py`'s agenda updates had silently never run).
- 2026-08-24 **new env var `agenda_markdown_dir`** (default `/tmp`) came out
  of this work the hard way: the first integration run of the fix rewrote the
  developer's own live `/tmp/2026-08-24.md` from a two-row fixture CSV,
  because `HOME`/`XDG_CONFIG_HOME` isolation does not redirect a hardcoded
  `/tmp`. The file was restored from the CSVs. Any test that runs a mutating
  tasks command through the binary must now isolate that variable first
  (`docs/testing.md`).
- 2026-08-24 **follow-up, not in scope here:** the other native mutation
  paths still leave the agenda alone — `brain habits skip`,
  `brain habits complete-managed-triage`, `brain habits revive`, and
  `apply_sync_rules.py --complete-managed-triage`. They can each reach the
  same sync through `brain tasks sync-agenda`; the triage skill was updated
  to call it instead of rewriting the markdown by hand. Worth a task if the
  staleness is felt.
- 2026-08-24 created. Filed after discovering `/tmp/2026-08-24.md` missing
  its MIT/Suggested/Cut/title sections mid-session; root cause traced to
  native completion never syncing the agenda file rather than a bug in
  `update_agenda_on_mutation.py`'s `touch` action (which was the initial
  suspect and was cleared by reading its source).
