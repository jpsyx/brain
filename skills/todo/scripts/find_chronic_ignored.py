#!/usr/bin/env python3
"""Find chronically-ignored tasks in tasks.csv.

Used by /triage's Step 7 (chronic-ignore sweep). Emits JSON Lines, one
matching task per line, sorted by severity (max days-since-touch first).

A task qualifies if `status != done`, its deadline is imminent or absent
(`due_date` is empty OR `today <= due_date <= today + 3`), and at least
one of:

Per an explicit user directive, chronic-ignore only nags about a dated
task once its `due_date` is within `CHRONIC_DUE_HORIZON_DAYS` (3 days) —
i.e. when it's genuinely about to slip. Anything further out is left
alone. (Note: the at-risk preview uses a wider 8-day window; chronic-
ignore deadwood is held to a tighter 3 days.) Past-due rows
(`due_date < today`) are owned by past-due triage (Steps 1-4), not this
sweep. Undated thin rows have no deadline to be "away from", so they stay
eligible — they are the truest captured-and-forgotten deadwood.

Qualifying reasons (at least one):

- **stale_21d** — `today - last_touched >= 21d`. The primary signal.
- **stuck_in_progress** — `status == in_progress` AND
  `today - last_touched >= 14d`. The user engaged once then walked away.
- **captured_forgotten** — `today - created_date >= 60d` AND
  `status == not_started` AND empty(`notes`, `estimated_duration`, `project`).
  A thin row that's old and untouched.

LLMs are bad at calendar math; use this script.

Usage:
    find_chronic_ignored.py                 # JSONL to stdout
    find_chronic_ignored.py --pretty        # one row per indented JSON object
    find_chronic_ignored.py --count         # just print the number of hits
"""
import argparse
import json
import sys
from datetime import date, datetime, timedelta
from pathlib import Path

from _csvlib import read_csv, tasks_csv

STALE_TOUCH_DAYS = 21
STUCK_IN_PROGRESS_DAYS = 14
CAPTURED_FORGOTTEN_AGE_DAYS = 60
# Don't surface a dated task in the chronic sweep until its deadline is
# this close. Tasks with a due_date further out than this are left alone
# (per user directive: only nag about chronic-ignored items whose
# deadline is within 3 days).
CHRONIC_DUE_HORIZON_DAYS = 3


def parse_date(s):
    s = (s or "").strip()
    if not s:
        return None
    try:
        return datetime.fromisoformat(s.split("T")[0]).date()
    except ValueError:
        return None


def classify(row, today: date):
    """Return a dict with reasons[] and supporting fields, or None if not chronic."""
    status = (row.get("status") or "").strip()
    if status == "done":
        return None
    # Backlog tasks are deliberately parked (and undated) — never flag them
    # as chronically ignored; the monthly backlog-review owns them.
    if status == "backlog":
        return None

    # A task parked with a future start_date is not yet actionable — it's
    # deliberately hidden until that date, so it can't be "ignored" yet.
    start = parse_date(row.get("start_date"))
    if start is not None and start > today:
        return None

    due = parse_date(row.get("due_date"))
    # Past-due triage (Steps 1-4) owns anything already late.
    if due is not None and due < today:
        return None
    # User directive: don't nag about chronic items whose deadline is still
    # far off. A dated task only surfaces once it's within
    # CHRONIC_DUE_HORIZON_DAYS of its due date; undated thin rows (no
    # deadline to be "away from") stay eligible.
    if due is not None and due > today + timedelta(days=CHRONIC_DUE_HORIZON_DAYS):
        return None

    last_touched = parse_date(row.get("last_touched"))
    created = parse_date(row.get("created_date"))
    days_since_touch = (today - last_touched).days if last_touched else None
    days_since_create = (today - created).days if created else None

    reasons = []
    if days_since_touch is not None and days_since_touch >= STALE_TOUCH_DAYS:
        reasons.append("stale_21d")
    if status == "in_progress" and days_since_touch is not None and days_since_touch >= STUCK_IN_PROGRESS_DAYS:
        reasons.append("stuck_in_progress")
    if (
        status == "not_started"
        and days_since_create is not None
        and days_since_create >= CAPTURED_FORGOTTEN_AGE_DAYS
        and not (row.get("notes") or "").strip()
        and not (row.get("estimated_duration") or "").strip()
        and not (row.get("project") or "").strip()
    ):
        reasons.append("captured_forgotten")

    if not reasons:
        return None

    return {
        "task_id": row.get("task_id"),
        "task_name": row.get("task_name"),
        "reasons": reasons,
        "days_since_touch": days_since_touch,
        "days_since_create": days_since_create,
        "status": status,
        "priority": row.get("priority"),
        "task_type": row.get("task_type"),
        "due_date": row.get("due_date") or "",
        "defer_count": int(row.get("defer_count") or 0),
        "project": row.get("project") or "",
        "hard_deadline": row.get("hard_deadline") == "true",
    }


def severity(hit):
    # Sort by max days-since-touch (fallback days-since-create), descending.
    return (hit["days_since_touch"] or 0, hit["days_since_create"] or 0)


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--pretty", action="store_true", help="indented JSON, one per line")
    p.add_argument("--count", action="store_true", help="print only the count")
    args = p.parse_args()

    today = date.today()
    cols, rows = read_csv(tasks_csv())
    if "last_touched" not in cols:
        print(
            "tasks.csv is missing the last_touched column. "
            "Run: python3 ~/.agents/skills/todo/scripts/apply_sync_rules.py --fix",
            file=sys.stderr,
        )
        return 2

    hits = [c for c in (classify(r, today) for r in rows) if c is not None]
    hits.sort(key=severity, reverse=True)

    if args.count:
        print(len(hits))
        return 0

    for hit in hits:
        if args.pretty:
            print(json.dumps(hit, indent=2, ensure_ascii=False))
        else:
            print(json.dumps(hit, ensure_ascii=False))

    return 0


if __name__ == "__main__":
    sys.exit(main())
