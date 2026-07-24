#!/usr/bin/env python3
"""Move a task to the backlog, or restore one from it.

The backlog is for tasks parked indefinitely — not abandoned (that's
`/todo remove`), just not on the active list right now. A backlogged task:

  - has `status = backlog`,
  - has its `due_date` and `start_date` CLEARED (a parked task has no
    schedule; `hard_deadline` is cleared too since it's meaningless without
    a due date),
  - is stamped with `backlogged_date = today`,
  - is hidden from every active view (is_visible_today is false) and never
    surfaced by the at-risk or chronic-ignore scans,
  - resurfaces only in the monthly triage's backlog-review, and
  - is auto-deleted once `backlogged_date` is >6 months old
    (see purge_old_backlog.py).

Restoring (`--restore`) flips status back to not_started and clears
backlogged_date; the user re-assigns a due_date/priority afterward.

This script handles ONE task's mechanical move. Project-level consequences
(backlogging a whole project, archiving it) are handled by the /todo and
/triage skills at the conversation layer — see SKILL.md "Backlog".

Usage:
    backlog_task.py <task_id_or_fuzzy>            # move to backlog
    backlog_task.py <task_id_or_fuzzy> --restore  # bring back to active
"""
import argparse
import subprocess
import sys
from pathlib import Path

from _csvlib import locate, today_iso, touch_row, write_csv

_UPDATE_AGENDA = Path(__file__).resolve().parent / "update_agenda_on_mutation.py"


def _update_agenda(task_id: str, action: str) -> None:
    if not _UPDATE_AGENDA.exists():
        return
    try:
        subprocess.run([sys.executable, str(_UPDATE_AGENDA), task_id, action], check=False)
    except OSError as e:
        print(f"[backlog_task] could not update agenda: {e}", file=sys.stderr)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("needle", help="task_id (T###) or fuzzy task_name match")
    p.add_argument("--restore", action="store_true",
                   help="restore a backlog task to active (status=not_started)")
    args = p.parse_args()

    path, cols, rows, idx, row = locate(args.needle)
    if "backlogged_date" not in cols:
        print(f"{path.name} has no backlogged_date column — habits can't be "
              f"backlogged ('{row.get('task_id')}').", file=sys.stderr)
        return 2

    tid = row.get("task_id") or ""
    name = row.get("task_name") or ""

    if args.restore:
        if rows[idx].get("status") != "backlog":
            print(f"{tid} is not in the backlog (status={rows[idx].get('status')}).",
                  file=sys.stderr)
            return 2
        rows[idx]["status"] = "not_started"
        rows[idx]["backlogged_date"] = ""
        touch_row(rows[idx])
        write_csv(path, cols, rows)
        print(f"restored from backlog: {tid}  {name}")
        print("  status: backlog → not_started (set a due_date/priority next)")
        _update_agenda(tid, "restore")
        return 0

    if rows[idx].get("status") == "backlog":
        print(f"{tid} is already in the backlog.", file=sys.stderr)
        return 0

    old_status = rows[idx].get("status")
    rows[idx]["status"] = "backlog"
    rows[idx]["backlogged_date"] = today_iso()
    rows[idx]["due_date"] = ""
    rows[idx]["start_date"] = ""
    rows[idx]["hard_deadline"] = "false"
    rows[idx]["waiting_since"] = ""
    touch_row(rows[idx])
    write_csv(path, cols, rows)
    print(f"moved to backlog: {tid}  {name}")
    print(f"  status: {old_status} → backlog  (due_date + start_date cleared)")
    print(f"  backlogged_date: {rows[idx]['backlogged_date']}")
    proj = (row.get("project") or "").strip()
    if proj:
        print(f"  ⚠ part of project '{proj}' — confirm whether to backlog the "
              f"whole project (see /todo SKILL.md Backlog rules).")
    _update_agenda(tid, "backlog")
    return 0


if __name__ == "__main__":
    sys.exit(main())
