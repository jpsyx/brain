#!/usr/bin/env python3
"""List backlog tasks (status == backlog).

Feeds the monthly triage backlog-review. Emits JSON Lines, one per task,
sorted oldest-backlogged first, with `days_in_backlog` so the reviewer can
weigh staleness. The skill decides which to resurface — this just lists.

Usage:
    list_backlog.py            # JSONL to stdout
    list_backlog.py --count    # just the count
    list_backlog.py --pretty   # human-readable lines
"""
import argparse
import json
import sys
from datetime import date

from _csvlib import parse_date, read_csv, tasks_csv


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--count", action="store_true")
    p.add_argument("--pretty", action="store_true")
    args = p.parse_args()

    today = date.today()
    _, rows = read_csv(tasks_csv())
    out = []
    for r in rows:
        if (r.get("status") or "").strip() != "backlog":
            continue
        bd = parse_date(r.get("backlogged_date"))
        out.append({
            "task_id": r.get("task_id"),
            "task_name": r.get("task_name"),
            "task_type": r.get("task_type"),
            "priority": r.get("priority"),
            "project": r.get("project") or "",
            "backlogged_date": r.get("backlogged_date") or "",
            "days_in_backlog": (today - bd).days if bd else None,
            "notes": r.get("notes") or "",
        })
    out.sort(key=lambda x: x["days_in_backlog"] or 0, reverse=True)

    if args.count:
        print(len(out))
        return 0
    for o in out:
        if args.pretty:
            print(f"{o['task_id']:>5} {o['priority']} {o['task_type']:<18} "
                  f"{o['days_in_backlog']}d | {o['task_name'][:50]}"
                  + (f"  [proj: {o['project']}]" if o['project'] else ""))
        else:
            print(json.dumps(o, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
