#!/usr/bin/env python3
"""Find tasks that have been stuck in status='waiting' too long.

A 'waiting' task is paused on an EXTERNAL party (a reply, a vendor, a
legal review) — not the user's fault, so deferring it carries no
penalty (see defer_task.py). But waiting forever is its own failure
mode: at some point the right move is to chase the external party.

This detector surfaces those: any row with `status == 'waiting'` whose
`waiting_since` is more than --threshold days ago (default 7). The
assistant (in /triage and /todo's agenda flow) uses it to prompt the
user to follow up and offer to create a check-in task.

LLMs are bad at calendar math; use this script.

Usage:
    find_stale_waiting.py                  # JSONL to stdout
    find_stale_waiting.py --pretty         # indented JSON, one per line
    find_stale_waiting.py --count          # just the number of hits
    find_stale_waiting.py --threshold 7    # days waiting before flagging
"""
import argparse
import json
import sys
from datetime import date, datetime
from pathlib import Path

from _csvlib import TASKS_CSV, read_csv

DEFAULT_THRESHOLD_DAYS = 7


def parse_date(s):
    s = (s or "").strip()
    if not s:
        return None
    try:
        return datetime.fromisoformat(s.split("T")[0]).date()
    except ValueError:
        return None


def classify(row, today: date, threshold: int):
    if (row.get("status") or "").strip() != "waiting":
        return None
    since = parse_date(row.get("waiting_since"))
    # No waiting_since recorded: still surface it (we can't tell how long,
    # which is itself worth fixing), with days_waiting = None.
    days_waiting = (today - since).days if since else None
    if days_waiting is not None and days_waiting <= threshold:
        return None
    return {
        "task_id": row.get("task_id"),
        "task_name": row.get("task_name"),
        "days_waiting": days_waiting,
        "waiting_since": row.get("waiting_since") or "",
        "priority": row.get("priority"),
        "task_type": row.get("task_type"),
        "due_date": row.get("due_date") or "",
        "see_also": row.get("see_also") or "",
        "notes": row.get("notes") or "",
    }


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--pretty", action="store_true", help="indented JSON, one per line")
    p.add_argument("--count", action="store_true", help="print only the count")
    p.add_argument("--threshold", type=int, default=DEFAULT_THRESHOLD_DAYS,
                   help=f"days waiting before flagging (default {DEFAULT_THRESHOLD_DAYS})")
    args = p.parse_args()

    today = date.today()
    cols, rows = read_csv(TASKS_CSV)
    if "waiting_since" not in cols:
        print(
            "tasks.csv is missing the waiting_since column. "
            "It should be added next to `status` per SCHEMA.json.",
            file=sys.stderr,
        )
        return 2

    hits = [c for c in (classify(r, today, args.threshold) for r in rows) if c is not None]
    # Longest wait first; unknown waits (None) sort last.
    hits.sort(key=lambda h: (h["days_waiting"] is None, -(h["days_waiting"] or 0)))

    if args.count:
        print(len(hits))
        return 0

    for hit in hits:
        print(json.dumps(hit, indent=2 if args.pretty else None, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
