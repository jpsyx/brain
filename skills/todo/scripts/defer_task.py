#!/usr/bin/env python3
"""Push a task's due_date forward and increment defer_count.

Usage:
    defer_task.py <task_id_or_fuzzy> +7d           # push 7 days
    defer_task.py <task_id_or_fuzzy> 2026-06-15    # set to absolute date
    defer_task.py <task_id_or_fuzzy> +7d --no-count # push without penalty

Side effects (defer-demote rule): a deferred task loses MIT status and,
if it was p0, drops to p1. Rationale: if it can wait, it's not urgent +
critical anymore. Lower priorities (p1-p4) keep their level but still
shed the MIT tag.

No-penalty defers: a defer does NOT count against the task when the
push isn't the user's fault, i.e. the task is *waiting* on something
out of their hands. In that case `defer_count` is left untouched and
the defer-demote rule is skipped (no point demoting a task that's only
late because it's blocked). This applies automatically when either:
  * `status == waiting` — waiting on external circumstances/people, or
  * `blocked_by` is non-empty — blocked on another task.
…and can be forced with `--no-count` for any other genuinely-not-our-
fault case. defer_count stays the "are we ignoring this?" signal; it
should only climb when the delay reflects our own avoidance.

Chunked tasks (see SKILL.md "Chunked tasks"): when the deferred row is
part of a chunk family, later chunks whose `due_date` would invert the
family order are cascaded forward so the order stays valid. Cascaded
chunks do NOT have their `defer_count` bumped — only the explicitly
deferred chunk does. Chunks already due later than the new date are
left alone.
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path
from _csvlib import (
    cascade_chunk_dates_forward, locate, shift_due, touch_row, write_csv,
)

_UPDATE_AGENDA = Path(__file__).resolve().parent / "update_agenda_on_mutation.py"


def _update_agenda(task_id: str, action: str) -> None:
    """Best-effort agenda side effect (see mark_done.py for rationale)."""
    if not _UPDATE_AGENDA.exists():
        return
    try:
        subprocess.run(
            [sys.executable, str(_UPDATE_AGENDA), task_id, action],
            check=False,
        )
    except OSError as e:
        print(f"[defer_task] could not update agenda: {e}", file=sys.stderr)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("needle", help="task_id or fuzzy task_name match")
    p.add_argument("when", help="+Nd or YYYY-MM-DD")
    p.add_argument("--no-count", action="store_true",
                   help="defer without incrementing defer_count or demoting "
                        "(for not-our-fault pushes). Auto-applied when the "
                        "task is waiting or blocked_by is set.")
    args = p.parse_args()

    path, cols, rows, idx, row = locate(args.needle)
    old_due = row.get("due_date") or ""
    old_defer = int(row.get("defer_count") or 0)
    old_priority = row.get("priority") or ""
    old_type = row.get("task_type") or ""

    # No-penalty defer: the push isn't the user's fault. True when forced
    # with --no-count, or automatically when the task is waiting (external)
    # or blocked_by another task. See module docstring.
    is_waiting = (row.get("status") or "") == "waiting"
    is_blocked = bool((row.get("blocked_by") or "").strip())
    no_count = args.no_count or is_waiting or is_blocked
    if no_count:
        reason = ("--no-count" if args.no_count and not (is_waiting or is_blocked)
                  else "waiting" if is_waiting else "blocked")

    when = args.when.strip()
    if re.fullmatch(r"\+\d+d", when):
        rows[idx]["due_date"] = shift_due(old_due, int(when[1:-1]))
    elif re.fullmatch(r"\d{4}-\d{2}-\d{2}", when):
        rows[idx]["due_date"] = when
    else:
        print(f"unknown date format: {when}", file=sys.stderr)
        return 2

    # only tasks.csv has defer_count; habits.csv doesn't (habits recur).
    # Skip the increment AND the defer-demote when this is a no-penalty
    # defer — a waiting/blocked task slipping isn't avoidance, so neither
    # the count nor the priority should be punished.
    if "defer_count" in cols and not no_count:
        rows[idx]["defer_count"] = str(old_defer + 1)

    # Defer-demote rule: drop mit tag; p0 → p1. Skipped for no-penalty defers.
    if not no_count:
        if "task_type" in cols and "mit" in old_type.split("|"):
            rows[idx]["task_type"] = "|".join(t for t in old_type.split("|") if t != "mit")
        if old_priority == "p0":
            rows[idx]["priority"] = "p1"

    touch_row(rows[idx])

    # Chunked-task cascade: push later siblings only when they'd invert the
    # family order. defer_count is intentionally NOT propagated.
    cascaded = cascade_chunk_dates_forward(rows, rows[idx])

    write_csv(path, cols, rows)
    new = rows[idx]
    print(f"deferred: {new['task_id']}  {new['task_name']}")
    print(f"  due_date: {old_due or '(none)'} → {new['due_date']}")
    if "defer_count" in cols:
        if no_count:
            print(f"  defer_count: {old_defer} (unchanged — no-penalty defer, {reason})")
        else:
            print(f"  defer_count: {old_defer} → {new['defer_count']}")
    if new.get("task_type", "") != old_type:
        print(f"  task_type: {old_type} → {new['task_type']}  (defer-demote: dropped mit)")
    if new.get("priority", "") != old_priority:
        print(f"  priority:  {old_priority} → {new['priority']}  (defer-demote: p0 → p1)")
    if cascaded:
        print(f"  ✦ cascaded {len(cascaded)} later chunk(s) (due_date only; defer_count untouched):")
        for r in cascaded:
            print(f"      {r['task_id']}  {r['task_name']}  → {r['due_date']}")
    if "defer_count" in cols and not no_count and int(new["defer_count"]) >= 3:
        print("  ⚠ deferred 3+ times — consider /todo remove or commit to a firmer date")
    # Linear link is LLM-mediated: this script can't reach the Linear MCP, so
    # the skill must push the changed properties to the mirrored issue. Keeping
    # due_date (and demoted priority) in sync is required, not optional. See
    # linear-link.md "Property sync".
    if new.get("linear_issue"):
        bits = [f"dueDate → {new['due_date']}"]
        if new.get("priority", "") != old_priority:
            bits.append(f"priority → {new['priority']}")
        print(f"  🔗 LINEAR: update issue {new['linear_issue']} ({', '.join(bits)}) "
              f"via mcp__linear__save_issue. (scripts can't reach Linear; the skill must.)")
    _update_agenda(new["task_id"], "defer")
    for r in cascaded:
        _update_agenda(r["task_id"], "defer")
    return 0


if __name__ == "__main__":
    sys.exit(main())
