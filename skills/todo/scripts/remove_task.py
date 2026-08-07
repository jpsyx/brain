#!/usr/bin/env python3
"""Remove one task (or, with --habit, one habit) through the protected writer.

`locate` searches tasks.csv *and* habits.csv, so a needle meant for a task can
land on a habit row. Deleting a habit row destroys the whole recurring chain —
every future occurrence goes with it — so it is refused unless the caller opts
in with `--habit`. Task cleanup passes (notably /triage) never pass that flag,
which is what keeps them structurally unable to delete a habit.
"""

import argparse
import sys

from _csvlib import habits_csv, locate, write_csv


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("needle", help="task_id (T###/H###) or unique fuzzy name")
    parser.add_argument(
        "--habit",
        action="store_true",
        help="required to remove a habit row, and refuses anything else",
    )
    args = parser.parse_args()

    path, columns, rows, index, row = locate(args.needle)
    is_habit = path == habits_csv()
    label = f"{row.get('task_id')}  {row.get('task_name')}"

    if is_habit and not args.habit:
        print(
            f"refusing to remove habit {label}: deleting a habit row destroys its "
            f"whole recurring chain, including every future occurrence.\n"
            f"Habits are never part of task cleanup. Pass --habit if you really "
            f"mean to retire this habit, or use defer_habit.py to push the next "
            f"occurrence out instead.",
            file=sys.stderr,
        )
        return 2
    if args.habit and not is_habit:
        print(
            f"--habit was passed but {label} is a task, not a habit; "
            f"re-run without --habit to remove it.",
            file=sys.stderr,
        )
        return 2

    removed = rows.pop(index)
    write_csv(path, columns, rows)
    kind = "removed habit" if is_habit else "removed"
    print(f"{kind}: {removed.get('task_id')}  {removed.get('task_name')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
