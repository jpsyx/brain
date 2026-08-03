#!/usr/bin/env python3
"""Remove one task or habit through the protected task-store writer."""

import argparse

from _csvlib import locate, write_csv


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("needle", help="task_id (T###/H###) or unique fuzzy name")
    args = parser.parse_args()

    path, columns, rows, index, row = locate(args.needle)
    removed = rows.pop(index)
    write_csv(path, columns, rows)
    print(f"removed: {removed.get('task_id')}  {removed.get('task_name')}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
