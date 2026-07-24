#!/usr/bin/env python3
"""Mark a task done. If the row is in habits.csv, also append the next occurrence.

Usage:
    mark_done.py <task_id_or_fuzzy>
"""
import argparse
import subprocess
import sys
from pathlib import Path
from _csvlib import (
    HABITS_CSV, TASKS_CSV, find_next_chunk, locate, new_habit_id, parse_chunk_name,
    today_iso, touch_row, write_csv,
)
from next_habit_occurrence import next_due

_UPDATE_AGENDA = Path(__file__).resolve().parent / "update_agenda_on_mutation.py"


def _update_agenda(task_id: str, action: str) -> None:
    """Run the agenda-update side effect. Best-effort — never propagates
    failures (the CSV mutation already succeeded; the agenda is downstream)."""
    if not _UPDATE_AGENDA.exists():
        return
    try:
        subprocess.run(
            [sys.executable, str(_UPDATE_AGENDA), task_id, action],
            check=False,
        )
    except OSError as e:
        print(f"[mark_done] could not update agenda: {e}", file=sys.stderr)


def _migrate_mit_to_next_chunk(rows, completed_row):
    """If `completed_row` is a chunk with `mit` in task_type, add `mit` to the
    next chunk in the same family. Returns the next-chunk row (or None).
    Mutates rows in place.
    """
    completed_type = completed_row.get("task_type") or ""
    if "mit" not in completed_type.split("|"):
        return None
    if not parse_chunk_name(completed_row.get("task_name") or ""):
        return None
    nxt_idx, nxt = find_next_chunk(rows, completed_row)
    if nxt is None:
        return None
    if (nxt.get("status") or "").strip() == "done":
        return None
    nxt_type = nxt.get("task_type") or ""
    parts = [t for t in nxt_type.split("|") if t]
    if "mit" in parts:
        return None
    parts.append("mit")
    rows[nxt_idx]["task_type"] = "|".join(parts)
    touch_row(rows[nxt_idx])
    return rows[nxt_idx]


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("needle", help="task_id or fuzzy task_name match")
    args = p.parse_args()

    path, cols, rows, idx, row = locate(args.needle)
    rows[idx]["status"] = "done"
    rows[idx]["completed_date"] = today_iso()
    touch_row(rows[idx])
    tid = row.get("task_id") or ""
    name = row.get("task_name") or "(unnamed)"

    if path == HABITS_CSV:
        # spawn next occurrence with a fresh H### id
        nd = next_due(row.get("due_date") or "", int(row.get("recur_interval") or 1),
                      row.get("recur_unit") or "days")
        new_row = dict(row)
        new_row["task_id"] = new_habit_id()
        new_row["status"] = "not_started"
        new_row["due_date"] = nd
        new_row["completed_date"] = ""
        new_row["created_date"] = today_iso()
        rows.append(new_row)
        write_csv(path, cols, rows)
        print(f"done: {tid}  {name}  (habit)")
        print(f"  next occurrence: {new_row['task_id']} due {nd}")
    else:
        # Chunked-task lifecycle: when a chunk with `mit` is marked done, the
        # MIT tag migrates to the next chunk so the user always has exactly one
        # actionable MIT for the chunked work. See SKILL.md "Chunked tasks".
        migrated = _migrate_mit_to_next_chunk(rows, rows[idx])
        write_csv(path, cols, rows)
        print(f"done: {tid}  {name}")
        if migrated is not None:
            print(f"  ✦ MIT migrated → {migrated['task_id']}  {migrated['task_name']}")
        # if linked to a project, that project's .METADATA.json tasks[] still
        # includes this task_id — sync will surface stale links later.
        if row.get("project"):
            print(f"  (still linked to project '{row.get('project')}'; run /todo sync to refresh)")
        # Linear link is LLM-mediated: this script can't reach the Linear MCP,
        # so the skill MUST close the mirrored issue. See linear-link.md.
        if row.get("linear_issue"):
            print(f"  🔗 LINEAR: close issue {row['linear_issue']} too — set its state to "
                  f"Done via mcp__linear__save_issue. (scripts can't reach Linear; the skill must.)")
    _update_agenda(tid, "done")
    return 0


if __name__ == "__main__":
    sys.exit(main())
