#!/usr/bin/env python3
"""Set (or clear) the `linear_issue` link on a tasks.csv row.

This is the structured writer for the task <-> Linear join key, analogous to
how project links are written. It only touches the local CSV; it does NOT talk
to Linear (standalone scripts can't reach the Linear MCP). The skill is
responsible for actually creating/closing the Linear issue and then calling
this to persist the identifier. See todo/references/linear-link.md.

Usage:
    set_linear_issue.py <task> AVA-123        # link task to issue AVA-123
    set_linear_issue.py <task> AVA-123 --url https://linear.app/...   # also store URL in see_also
    set_linear_issue.py <task> ""             # clear the link
    set_linear_issue.py <task> --clear        # clear the link

<task> is a T### id, bare integer, or task_name fragment (same resolution as
every other /todo command). Habits are rejected — only tasks.csv rows link to
Linear.
"""
import argparse
import sys
from _csvlib import HABITS_CSV, TASKS_CSV, locate, touch_row, write_csv


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("needle", help="task_id or fuzzy task_name match (tasks.csv only)")
    p.add_argument("issue", nargs="?", default=None,
                   help="Linear issue identifier (e.g. AVA-123). Empty string or --clear to unlink.")
    p.add_argument("--url", default="",
                   help="optional Linear issue URL to store in see_also")
    p.add_argument("--clear", action="store_true", help="clear the link")
    args = p.parse_args()

    path, cols, rows, idx, row = locate(args.needle)
    if path == HABITS_CSV:
        print("set_linear_issue: habits don't link to Linear — only tasks.csv rows do.",
              file=sys.stderr)
        return 2

    if "linear_issue" not in cols:
        cols = list(cols) + ["linear_issue"]

    clearing = args.clear or (args.issue is not None and args.issue.strip() == "")
    if not clearing and not args.issue:
        print("set_linear_issue: provide an issue identifier, an empty string, or --clear.",
              file=sys.stderr)
        return 2

    tid = row.get("task_id") or ""
    name = row.get("task_name") or "(unnamed)"

    if clearing:
        rows[idx]["linear_issue"] = ""
        touch_row(rows[idx])
        write_csv(path, cols, rows)
        print(f"unlinked: {tid}  {name}  (Linear link cleared)")
        return 0

    issue = args.issue.strip()
    rows[idx]["linear_issue"] = issue
    if args.url:
        existing = (rows[idx].get("see_also") or "").strip()
        if args.url not in existing:
            rows[idx]["see_also"] = f"{existing} {args.url}".strip()
    touch_row(rows[idx])
    write_csv(path, cols, rows)
    print(f"linked: {tid}  {name}  🔗 {issue}")
    if "code" not in (row.get("task_type") or "").split("|"):
        print("  ⚠ note: this task isn't tagged `code`. Linear links are for code work.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
