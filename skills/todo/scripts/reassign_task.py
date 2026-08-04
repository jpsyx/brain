#!/usr/bin/env python3
"""Explicitly assign a task or habit to another portable workspace member."""

import argparse
import sys

from _csvlib import locate, touch_row, validate_assigned_to, write_csv


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("needle", help="task ID or unambiguous task-name fragment")
    parser.add_argument("assigned_to", help="portable workspace user ID")
    args = parser.parse_args()

    assigned_to = validate_assigned_to(args.assigned_to)
    path, columns, rows, index, row = locate(args.needle)
    if "assigned_to" not in columns:
        columns = list(columns) + ["assigned_to"]
    rows[index]["assigned_to"] = assigned_to
    touch_row(rows[index])
    write_csv(path, columns, rows)
    print(
        f"assigned: {row.get('task_id')}  {row.get('task_name')}  → {assigned_to}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
