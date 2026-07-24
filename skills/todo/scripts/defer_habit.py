#!/usr/bin/env python3
"""Skip the next occurrence of a habit.

Advances the habit's due_date by one recurrence interval (or N, with
--occurrences). Uses the same anchor-to-due-with-catch-up math as
mark_done's spawn step, so a Monday-weekly habit stays on Mondays
after skipping. No `completed_date` is recorded — the skipped
instance is simply not done.

Habits don't have `defer_count` (the recurrence is the deferral
mechanism), so nothing is incremented.

Usage:
    defer_habit.py <habit_id_or_fuzzy>                  # skip 1 occurrence
    defer_habit.py <habit_id_or_fuzzy> --occurrences 2  # skip 2
"""
import argparse
import subprocess
import sys
from pathlib import Path

from _csvlib import locate, write_csv
from next_habit_occurrence import next_due

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
        print(f"[defer_habit] could not update agenda: {e}", file=sys.stderr)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("needle", help="habit_id (H###) or fuzzy task_name match")
    p.add_argument(
        "--occurrences",
        type=int,
        default=1,
        help="how many occurrences to skip (default: 1)",
    )
    args = p.parse_args()

    path, cols, rows, idx, row = locate(args.needle)
    if path.name != "habits.csv":
        print(
            f"defer_habit only operates on habits.csv rows; {row['task_id']} "
            "is in tasks.csv. Use defer_task.py instead.",
            file=sys.stderr,
        )
        return 2

    interval = int(row["recur_interval"])
    unit = row["recur_unit"]
    old_due = row["due_date"]

    new_due = old_due
    for _ in range(args.occurrences):
        new_due = next_due(new_due, interval, unit)

    rows[idx]["due_date"] = new_due
    write_csv(path, cols, rows)

    new = rows[idx]
    print(f"deferred habit: {new['task_id']}  {new['task_name']}")
    print(f"  due_date: {old_due} → {new['due_date']}  (skipped {args.occurrences} × {interval} {unit})")
    _update_agenda(new["task_id"], "defer")
    return 0


if __name__ == "__main__":
    sys.exit(main())
