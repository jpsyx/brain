#!/usr/bin/env python3
"""List every tasks.csv row that carries a `linear_issue` link.

This is the structured reader the skills use to reconcile task <-> Linear state
(in /todo sync-linear and /triage). It only reads the local CSV; the caller is
responsible for calling the Linear MCP (get_issue) on each identifier and
acting on any drift. See todo/references/linear-link.md.

Outputs one JSON object per linked task with the fields needed to reconcile:
    task_id, task_name, status, linear_issue, task_type, priority, project

Usage:
    list_linked_tasks.py                 # one JSON object per line (jsonl)
    list_linked_tasks.py --open-only     # only rows where status != done
    list_linked_tasks.py --count         # just the number
    list_linked_tasks.py --pretty        # human-readable table
"""
import argparse
import json
import sys
from _csvlib import read_csv, tasks_csv

FIELDS = ["task_id", "task_name", "status", "linear_issue",
          "task_type", "priority", "project"]


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--open-only", action="store_true",
                   help="only rows where status != done")
    p.add_argument("--count", action="store_true", help="print only the count")
    p.add_argument("--pretty", action="store_true", help="human-readable output")
    args = p.parse_args()

    _, rows = read_csv(tasks_csv())
    linked = [r for r in rows if (r.get("linear_issue") or "").strip()]
    if args.open_only:
        linked = [r for r in linked if (r.get("status") or "") != "done"]

    if args.count:
        print(len(linked))
        return 0

    if args.pretty:
        if not linked:
            print("no linked tasks")
            return 0
        for r in linked:
            print(f"{r.get('task_id'):>5}  {r.get('linear_issue'):<10}  "
                  f"[{r.get('status')}]  {r.get('task_name')}")
        return 0

    for r in linked:
        print(json.dumps({k: r.get(k, "") for k in FIELDS}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
