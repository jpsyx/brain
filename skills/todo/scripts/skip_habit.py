#!/usr/bin/env python3
"""Skip a habit for today, with semantics that depend on the habit's cadence.

The rule (see /todo SKILL.md "Skipping a habit"):

- **Daily habit** (recur_interval == 1 AND recur_unit == "days"): a plain skip
  means **mark today's occurrence done**. Tomorrow the habit is back anyway, so
  "skip it today" is functionally "it's handled for today" — exactly what
  brain tasks complete does (records completed_date=today and spawns tomorrow's occurrence).

- **Non-daily habit** (weekly, monthly, every-N-days, …): a plain skip does
  **NOT** mark it done. It simply **defers the due_date to tomorrow**
  (today + 1 day) by default. Skipping a non-daily habit is a one-day defer
  unless a later day is specified.

- **--until YYYY-MM-DD** (either cadence): "skip until a certain day" — defer
  the due_date to that day, never marking it done. Must be strictly after today.

This subsumes the old "skip daily triage" special case: Morning Triage is a
daily habit, so `skip_habit.py "Morning Triage"` marks it done for today, which
is what stops the `tasks` TUI from nagging.

Usage:
    skip_habit.py <habit_id_or_fuzzy>                  # cadence-aware skip
    skip_habit.py <habit_id_or_fuzzy> --until 2026-07-20
"""
import argparse
import subprocess
import sys
from datetime import date, timedelta
from pathlib import Path

from _csvlib import (
    HABITS_CSV, locate, new_habit_id, parse_date, today_iso, touch_row, write_csv,
)
from next_habit_occurrence import next_due

_UPDATE_AGENDA = Path(__file__).resolve().parent / "update_agenda_on_mutation.py"


def _update_agenda(task_id: str, action: str) -> None:
    """Best-effort agenda side effect."""
    if not _UPDATE_AGENDA.exists():
        return
    try:
        subprocess.run(
            [sys.executable, str(_UPDATE_AGENDA), task_id, action],
            check=False,
        )
    except OSError as e:
        print(f"[skip_habit] could not update agenda: {e}", file=sys.stderr)


def _is_daily(row: dict) -> bool:
    try:
        interval = int(row.get("recur_interval") or 0)
    except ValueError:
        interval = 0
    unit = (row.get("recur_unit") or "").strip().lower()
    return interval == 1 and unit == "days"


def _mark_done(path, cols, rows, idx) -> int:
    """Daily-habit skip: complete today's occurrence + spawn the next one."""
    row = rows[idx]
    rows[idx]["status"] = "done"
    rows[idx]["completed_date"] = today_iso()
    touch_row(rows[idx])
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
    print(f"skipped (daily → marked done): {row['task_id']}  {row['task_name']}")
    print(f"  next occurrence: {new_row['task_id']} due {nd}")
    _update_agenda(row["task_id"], "done")
    return 0


def _defer(path, cols, rows, idx, new_due: str, label: str) -> int:
    """Non-daily skip (or --until): move the due_date, never mark done."""
    row = rows[idx]
    old_due = row.get("due_date") or "(none)"
    rows[idx]["due_date"] = new_due
    touch_row(rows[idx])
    write_csv(path, cols, rows)
    print(f"skipped ({label}): {row['task_id']}  {row['task_name']}")
    print(f"  due_date: {old_due} → {new_due}")
    _update_agenda(row["task_id"], "defer")
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("needle", help="habit_id (H###) or fuzzy task_name match")
    p.add_argument(
        "--until",
        metavar="YYYY-MM-DD",
        help="defer the habit until this day (never marks done); must be after today",
    )
    args = p.parse_args()

    path, cols, rows, idx, row = locate(args.needle)
    if path != HABITS_CSV:
        print(
            f"skip_habit only operates on habits.csv rows; {row['task_id']} is a "
            "task. Skipping is a habit concept — use defer_task.py / brain tasks complete "
            "for tasks.",
            file=sys.stderr,
        )
        return 2

    today = date.today()

    if args.until:
        try:
            target = parse_date(args.until)
        except (ValueError, TypeError):
            print(f"--until must be YYYY-MM-DD, got {args.until!r}", file=sys.stderr)
            return 2
        if target <= today:
            print(f"--until must be strictly after today ({today.isoformat()}); "
                  f"got {target.isoformat()}", file=sys.stderr)
            return 2
        return _defer(path, cols, rows, idx, target.isoformat(),
                      f"until {target.isoformat()}")

    if _is_daily(row):
        return _mark_done(path, cols, rows, idx)

    tomorrow = (today + timedelta(days=1)).isoformat()
    return _defer(path, cols, rows, idx, tomorrow, "non-daily → deferred to tomorrow")


if __name__ == "__main__":
    sys.exit(main())
