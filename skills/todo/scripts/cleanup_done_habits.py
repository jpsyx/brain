#!/usr/bin/env python3
"""Delete habits.csv rows where status=done AND completed_date <= today - 7d.

Run as part of `/second-brain reindex` and `/todo reindex` so habits.csv stays
short. The completed-but-recent rows stay for a week so the user can
inspect / undo.
"""
import csv
import sys
from datetime import date, datetime, timedelta
from pathlib import Path

HABITS = Path.home() / "brain" / "tasks" / "habits.csv"
CUTOFF = date.today() - timedelta(days=7)


def parse_date(s: str):
    s = (s or "").strip()
    if not s:
        return None
    return datetime.fromisoformat(s.split("T")[0]).date()


def main() -> int:
    if not HABITS.exists():
        print(f"no {HABITS}", file=sys.stderr)
        return 0
    with open(HABITS, newline="") as f:
        reader = csv.DictReader(f)
        columns = reader.fieldnames
        rows = list(reader)

    keep, drop = [], []
    for r in rows:
        if r.get("status") == "done":
            cd = parse_date(r.get("completed_date") or "")
            if cd is not None and cd <= CUTOFF:
                drop.append(r)
                continue
        keep.append(r)

    if not drop:
        print(f"cleanup: no done habits older than {CUTOFF}; kept {len(keep)} rows")
        return 0

    with open(HABITS, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=columns, quoting=csv.QUOTE_MINIMAL)
        w.writeheader()
        for r in keep:
            w.writerow(r)
    print(f"cleanup: dropped {len(drop)} done habit(s) older than {CUTOFF}; kept {len(keep)} rows")
    for r in drop:
        print(f"  - {r.get('task_name')} (completed {r.get('completed_date')})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
