#!/usr/bin/env python3
"""Bump a task's last_touched to today without changing anything else.

Used by /triage's chronic-ignore "revive" action: the user explicitly
acknowledges a stale task so it won't be flagged again until it goes
quiet for another 21+ days. No status, priority, due_date, or defer_count
change — just the touch.

Habits.csv rows have no last_touched column; this script errors out on
those instead of silently no-opping.

Usage:
    touch_task.py <task_id_or_fuzzy>
"""
import argparse
import subprocess
import sys
from pathlib import Path

from _csvlib import locate, today_iso, touch_row, write_csv

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
        print(f"[touch_task] could not update agenda: {e}", file=sys.stderr)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("needle", help="task_id (T###) or fuzzy task_name match")
    args = p.parse_args()

    path, cols, rows, idx, row = locate(args.needle)
    if "last_touched" not in cols:
        print(
            f"{path.name} has no last_touched column (habits have inherent recurrence; "
            f"task '{row.get('task_id')}' isn't a tasks.csv row).",
            file=sys.stderr,
        )
        return 2

    old = (rows[idx].get("last_touched") or "").strip() or "(never)"
    touch_row(rows[idx])
    write_csv(path, cols, rows)
    print(f"touched: {row.get('task_id')}  {row.get('task_name')}")
    print(f"  last_touched: {old} → {rows[idx]['last_touched']}")
    _update_agenda(row.get("task_id") or "", "touch")
    return 0


if __name__ == "__main__":
    sys.exit(main())
