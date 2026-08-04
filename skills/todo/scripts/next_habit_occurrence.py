#!/usr/bin/env python3
"""Compute the next-occurrence row for a recurring habit.

Anchor-to-due **with catch-up**: next_due = original_due + N × interval,
where N is the smallest integer that makes next_due STRICTLY > today.
A Monday-weekly habit completed 8 weeks late still lands on a future
Monday. A daily habit completed today schedules tomorrow.

Reads a habit row (current occurrence) from stdin as JSON and writes the
next occurrence row to stdout as JSON. The next row keeps every column
except: new UUIDv4 task_uuid, new H### task_id, status=not_started,
completed_date='', created_date=today, due_date = computed as above.

LLMs are bad at calendar math; use this script.

Usage:
    cat habit.json | next_habit_occurrence.py
    next_habit_occurrence.py --due 2026-06-10 --interval 3 --unit days
"""
from __future__ import annotations

import argparse
import json
import sys
from datetime import date, datetime, timedelta

from _csvlib import new_habit_id, new_uuid

# month math without dateutil
def add_months(d: date, months: int) -> date:
    m = d.month - 1 + months
    y = d.year + m // 12
    m = m % 12 + 1
    # clamp day to last valid day of target month
    for day in (d.day, 28, 29, 30, 31):
        try:
            return date(y, m, min(day, d.day))
        except ValueError:
            continue
    return date(y, m, 28)


def add_interval(d: date, interval: int, unit: str) -> date:
    if unit == "days":
        return d + timedelta(days=interval)
    if unit == "weeks":
        return d + timedelta(weeks=interval)
    if unit == "months":
        return add_months(d, interval)
    raise ValueError(f"unknown recur_unit: {unit!r}")


def parse_date(s: str) -> date:
    s = (s or "").strip()
    if not s:
        raise ValueError("due_date is empty")
    return datetime.fromisoformat(s.split("T")[0]).date()


def next_due(due: str, interval: int, unit: str, today: date | None = None) -> str:
    """Anchor to original due + interval, then fast-forward by `interval` units
    until strictly after today. Guarantees: result > today."""
    today = today or date.today()
    d = add_interval(parse_date(due), interval, unit)
    # safety cap: 600 iterations covers daily habits stale by >1.5y, weekly
    # by ~11y, monthly by 50y. Bail rather than spin forever.
    for _ in range(600):
        if d > today:
            return d.isoformat()
        d = add_interval(d, interval, unit)
    raise RuntimeError(
        f"could not fast-forward past today after 600 steps "
        f"(due={due}, interval={interval}, unit={unit})"
    )


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--due", help="original due_date (YYYY-MM-DD)")
    p.add_argument("--interval", type=int, help="recur_interval")
    p.add_argument("--unit", choices=["days", "weeks", "months"], help="recur_unit")
    p.add_argument("--row", help="path to JSON file with full habit row (alternative to flags)")
    args = p.parse_args()

    # CLI-only mode: print the next due date and exit
    if args.due and args.interval and args.unit and not args.row:
        print(next_due(args.due, args.interval, args.unit))
        return 0

    if args.row:
        row = json.loads(open(args.row).read())
    elif not sys.stdin.isatty():
        stdin_data = sys.stdin.read().strip()
        if not stdin_data:
            p.error("provide --row, non-empty stdin JSON, or --due + --interval + --unit")
        row = json.loads(stdin_data)
    else:
        p.error("provide --row, stdin JSON, or --due + --interval + --unit")

    nd = next_due(row["due_date"], int(row["recur_interval"]), row["recur_unit"])
    new = dict(row)
    new["task_uuid"] = new_uuid()
    new["task_id"] = new_habit_id()
    new["status"] = "not_started"
    new["due_date"] = nd
    new["completed_date"] = ""
    new["created_date"] = date.today().isoformat()
    print(json.dumps(new, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
